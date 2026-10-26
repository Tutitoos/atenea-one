#!/usr/bin/env python3
# Copyright 2026 Tutitoos
# SPDX-License-Identifier: Apache-2.0
"""Herramientas independientes de Atenea; sockets persistentes durante los bloqueos."""
import errno
import json
import selectors
import socket
import subprocess
import sys
import threading
import time
import uuid


def service(host):
    family = socket.AF_INET6 if ":" in host else socket.AF_INET
    selector = selectors.DefaultSelector()
    for port in (443, 3478, 8081):
        udp = socket.socket(family, socket.SOCK_DGRAM)
        udp.bind((host, port))
        selector.register(udp, selectors.EVENT_READ)
    tcp = socket.socket(family, socket.SOCK_STREAM)
    tcp.setsockopt(socket.SOL_SOCKET, socket.SO_REUSEADDR, 1)
    tcp.bind((host, 8081))
    tcp.listen()
    selector.register(tcp, selectors.EVENT_READ)
    print("READY", flush=True)

    def echo(connection):
        with connection:
            while data := connection.recv(4096):
                connection.sendall(data)

    while True:
        for event, _ in selector.select():
            sock = event.fileobj
            if sock is tcp:
                connection, _ = sock.accept()
                threading.Thread(target=echo, args=(connection,), daemon=True).start()
            else:
                data, address = sock.recvfrom(4096)
                sock.sendto(json.dumps({"echo": data.decode(), "endpoint": address[:2]}).encode(), address)


def send_udp(sock, data, target):
    try:
        sock.sendto(data, target)
        return True
    except OSError as error:
        # Linux devuelve EPERM al descartar en OUTPUT; equivale a no entrega.
        if error.errno != errno.EPERM:
            raise
        return False


def worker(host, port):
    family = socket.AF_INET6 if ":" in host else socket.AF_INET
    udp = socket.socket(family, socket.SOCK_DGRAM)
    udp.bind(("::" if family == socket.AF_INET6 else "0.0.0.0", port))
    udp.setblocking(False)
    # La primera conexión IPv6 incluye resolver vecinos, no solo el RTT TCP.
    tcp = socket.create_connection((host, 8081), timeout=10)
    tcp.settimeout(2)
    for line in sys.stdin:
        command = json.loads(line)
        kind = command[0]
        marker = uuid.uuid4().hex.encode()
        if kind == "tcp":
            tcp.sendall(marker)
            data = bytearray()
            while len(data) < len(marker):
                part = tcp.recv(len(marker) - len(data))
                assert part, "TCP cerrado inesperadamente"
                data.extend(part)
            result = bytes(data) == marker
        elif kind == "udp":
            target = (host, command[1])
            deadline = time.monotonic() + 5
            sent = send_udp(udp, marker, target)
            result = None
            while sent and time.monotonic() < deadline:
                try:
                    data, address = udp.recvfrom(4096)
                    # El mismo socket conserva datagramas P2P de la fase anterior.
                    if address[:2] != target:
                        continue
                    reply = json.loads(data)
                    if reply["echo"] == marker.decode():
                        result = reply["endpoint"]
                        break
                except BlockingIOError:
                    time.sleep(0.005)
        elif kind == "drain":
            while True:
                try:
                    udp.recvfrom(4096)
                except BlockingIOError:
                    break
            result = True
        elif kind == "peer":
            target, peer = command[1:]
            result = False
            deadline = time.monotonic() + 1.1
            while time.monotonic() < deadline:
                send_udp(udp, b"peer", tuple(target))
                time.sleep(0.03)
                try:
                    data, _ = udp.recvfrom(4096)
                    result |= data == b"peer"
                except BlockingIOError:
                    pass
        else:
            raise ValueError(command)
        print(json.dumps(result), flush=True)


def controller(host, topology):
    processes = [subprocess.Popen(["ip", "netns", "exec", ns, "python3", __file__,
                                  "worker", host, str(port)], stdin=subprocess.PIPE,
                                 stdout=subprocess.PIPE, text=True)
                 for ns, port in (("a", 42000), ("b", 42001))]

    def ask(index, command):
        process = processes[index]
        process.stdin.write(json.dumps(command) + "\n")
        process.stdin.flush()

    def answer(index):
        line = processes[index].stdout.readline()
        assert line, "Cliente terminado inesperadamente"
        return json.loads(line)

    def request(index, command):
        ask(index, command)
        return answer(index)

    def block(mode):
        subprocess.run(["bash", "-ec", "source tests/network-lab/nat/lib.sh; " + mode], check=True)

    def peer(endpoints, expected):
        for index in range(2):
            request(index, ["drain"])
        for index in range(2):
            ask(index, ["peer", endpoints[1-index], index])
        actual = [answer(0), answer(1)]
        assert actual == [expected, expected], ("p2p", actual, expected)
        print("  OK P2P sockets persistentes:", actual, flush=True)

    def inet(udp_ports):
        for index in range(2):
            for port in (443, 3478, 8081):
                received = request(index, ["udp", port]) is not None
                assert received == (port in udp_ports), (index, port, received, udp_ports)
            assert request(index, ["tcp"])
        print(f"  OK a/b ↔ inet: UDP={udp_ports}, TCP 8081 establecido permitido", flush=True)

    try:
        endpoints = [request(index, ["udp", 3478]) for index in range(2)]
        assert all(endpoints), endpoints
        if topology.startswith("samelan"):
            # Candidato local: incluso hairpin_off debe permitir el acceso directo LAN.
            endpoints = [["192.168.1.10", 42000], ["192.168.1.11", 42001]]
        reachable = topology != "symmetric"
        for cycle in range(2):
            print(f" Ciclo {cycle + 1}, endpoints={endpoints}", flush=True)
            block("net_unblock_all; net_unblock_all")
            inet([443, 3478, 8081])
            peer(endpoints, reachable)
            block("net_block_p2p; net_block_p2p")
            peer(endpoints, False)
            inet([443, 3478, 8081])
            counters = json.loads(subprocess.check_output(
                ["ip", "netns", "exec", "a", "nft", "-j", "list", "table", "inet", "relay_lab"],
                text=True))
            drops = sum(expression["counter"]["packets"]
                        for item in counters["nftables"] if "rule" in item
                        for expression in item["rule"]["expr"] if "counter" in expression)
            assert drops > 0, "El filtro P2P debe descartar intentos incluso en symmetric"
            print(f"  OK descartes P2P observados en a: {drops}", flush=True)
            block("net_block_udp; net_block_udp")
            peer(endpoints, False)
            inet([])
            block("net_block_relay_udp; net_block_relay_udp")
            inet([3478, 8081])
            peer(endpoints, reachable)
            block("net_unblock_all; net_unblock_all")
            inet([443, 3478, 8081])
            peer(endpoints, reachable)
        print("OK reglas repetibles y restauración sin reiniciar sockets", flush=True)
    finally:
        for process in processes:
            process.stdin.close()
        for process in processes:
            process.wait(timeout=5)
            assert process.returncode == 0, process.returncode


if __name__ == "__main__":
    mode = sys.argv[1]
    if mode == "serve":
        service(sys.argv[2])
    elif mode == "worker":
        worker(sys.argv[2], int(sys.argv[3]))
    else:
        controller(*sys.argv[2:])
