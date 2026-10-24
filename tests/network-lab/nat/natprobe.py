#!/usr/bin/env python3
"""Sonda UDP independiente de Atenea; endpoints JSON [dirección, puerto]."""
import argparse
import json
import select
import socket
import subprocess
import sys
import time
from pathlib import Path

SCRIPT = str(Path(__file__).resolve())


def udp(ip, port):
    sock = socket.socket(socket.AF_INET6 if ":" in ip else socket.AF_INET, socket.SOCK_DGRAM)
    if sock.family == socket.AF_INET6:
        sock.setsockopt(socket.IPPROTO_IPV6, socket.IPV6_V6ONLY, 1)
    sock.bind(("::" if sock.family == socket.AF_INET6 else "0.0.0.0", port))
    sock.settimeout(5)
    return sock


def observed(sock, ip, port):
    sock.sendto(b"observe", (ip, port))
    data, source = sock.recvfrom(1024)
    if source[:2] != (ip, port):
        raise RuntimeError(f"Respuesta de origen inesperado: {source}")
    return json.loads(data)


def reflect():
    sockets = [udp(ip, port) for ip in ("0.0.0.0", "::") for port in (3478, 3479)]
    print("ready", flush=True)
    while True:
        for sock in select.select(sockets, [], [])[0]:
            data, source = sock.recvfrom(1024)
            if data == b"observe":
                sock.sendto(json.dumps(list(source[:2])).encode(), source)


def mapping(args):
    with udp(args.ip, args.port) as sock:
        first = observed(sock, args.ip, 3478)
        second = observed(sock, args.ip, 3479)
        print(json.dumps({"mapping": "independent" if first == second else "dependent",
                          "endpoints": [first, second]}), flush=True)


def worker(args):
    with udp(args.ip, args.port) as sock:
        print(json.dumps(observed(sock, args.ip, 3478)), flush=True)
        peer = json.loads(sys.stdin.readline())
        deadline = time.monotonic() + 4
        next_send = 0
        received = False
        while time.monotonic() < deadline:
            try:
                now = time.monotonic()
                if now >= next_send:
                    next_send = now + 1
                    sock.sendto(f"natprobe-punch:{args.port}".encode(), tuple(peer["endpoint"]))
                if select.select([sock], [], [], 0.05)[0]:
                    data, _ = sock.recvfrom(1024)
                    received |= data == f"natprobe-punch:{peer['token']}".encode()
            except ConnectionRefusedError:
                # Sin hairpin el router puede devolver ICMP port-unreachable.
                time.sleep(0.05)
        print(json.dumps(received), flush=True)


def spawn(ns, *args):
    return subprocess.Popen(["ip", "netns", "exec", ns, "python3", SCRIPT, *args],
                            stdin=subprocess.PIPE, stdout=subprocess.PIPE, text=True)


def line(proc):
    if not select.select([proc.stdout], [], [], 6)[0]:
        raise RuntimeError("La sonda hija no respondió a tiempo")
    value = proc.stdout.readline()
    if not value:
        raise RuntimeError("La sonda hija terminó sin resultado")
    return json.loads(value)


def finish(proc):
    if proc.poll() is None:
        proc.terminate()
    proc.wait(timeout=3)


def pair(args):
    children = []
    try:
        for ns, port in (("a", 42000), ("b", 42001)):
            children.append(spawn(ns, "worker", "--ip", args.ip, "--port", str(port)))
        endpoints = [line(proc) for proc in children]
        for i, proc in enumerate(children):
            proc.stdin.write(json.dumps({"endpoint": endpoints[1-i],
                                        "token": 42001 if i == 0 else 42000}) + "\n")
            proc.stdin.flush()
        results = [line(proc) for proc in children]
        for proc in children:
            if proc.wait(timeout=3) != 0:
                raise RuntimeError("Falló el cliente UDP")
        print(json.dumps({"endpoints": endpoints, "received": results, "success": all(results),
                          "duration_secs": 4, "send_interval_secs": 1}))
    finally:
        for proc in children:
            finish(proc)


def listen(args):
    with udp(args.ip, args.port) as sock:
        print("true", flush=True)
        sock.settimeout(0.8)
        try:
            data, _ = sock.recvfrom(1024)
            received = data == b"unsolicited"
        except socket.timeout:
            received = False
        print(json.dumps(received), flush=True)


def unsolicited(args):
    proc = spawn("b", "listen", "--ip", args.ip, "--port", "43001")
    try:
        line(proc)
        subprocess.run(["ip", "netns", "exec", "inet", "python3", SCRIPT,
                        "send", "--ip", args.ip, "--port", "43001"], check=True)
        received = line(proc)
        if proc.wait(timeout=3) != 0:
            raise RuntimeError("Falló el receptor UDP")
        print(json.dumps({"blocked": not received}))
    finally:
        finish(proc)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("mode", choices=("reflect", "mapping", "worker", "pair", "listen", "send", "unsolicited"))
    parser.add_argument("--ip", default="198.51.100.10")
    parser.add_argument("--port", type=int, default=41000)
    args = parser.parse_args()
    if args.mode == "send":
        with udp(args.ip, 0) as sock:
            for _ in range(3):
                sock.sendto(b"unsolicited", (args.ip, args.port))
                time.sleep(0.05)
    else:
        {"reflect": reflect, "mapping": lambda: mapping(args), "worker": lambda: worker(args),
         "pair": lambda: pair(args), "listen": lambda: listen(args),
         "unsolicited": lambda: unsolicited(args)}[args.mode]()


if __name__ == "__main__":
    main()
