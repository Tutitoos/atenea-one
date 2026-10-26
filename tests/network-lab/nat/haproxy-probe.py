#!/usr/bin/env python3
# Copyright 2026 Tutitoos
# SPDX-License-Identifier: Apache-2.0
"""Relay TLS de eco de prueba, valida PROXY v2 antes del handshake."""
import argparse
import ipaddress
import json
import socket
import socketserver
import ssl
import struct
import threading
import time

SIGNATURE = b"\r\n\r\n\x00\r\nQUIT\n"


def exact(sock, size):
    data = bytearray()
    while len(data) < size:
        part = sock.recv(size - len(data))
        if not part:
            raise ValueError("PROXY v2 truncado")
        data.extend(part)
    return bytes(data)


def context(ca):
    ctx = ssl.create_default_context(cafile=ca)
    ctx.minimum_version = ssl.TLSVersion.TLSv1_3
    ctx.set_alpn_protocols(["atenea-relay/2"])
    return ctx


def hello(ca, name):
    outgoing = ssl.MemoryBIO()
    ctx = context(ca)
    # Solo genera bytes, sin handshake: permite el caso deliberado sin SNI.
    ctx.check_hostname = False
    connection = ctx.wrap_bio(ssl.MemoryBIO(), outgoing, server_hostname=name)
    try:
        connection.do_handshake()
    except ssl.SSLWantReadError:
        pass
    return outgoing.read()


def check(host, ca, expected):
    def relay():
        with socket.create_connection((host, 443), timeout=3) as raw:
            with context(ca).wrap_socket(raw, server_hostname="relay.atenea.one") as tls:
                assert tls.selected_alpn_protocol() == "atenea-relay/2"
                tls.sendall(b"echo")
                result = json.loads(tls.recv(4096))
                assert result == {"source": expected, "echo": "echo"}, result
                print("OK SNI+ALPN, TLS 1.3, PROXY v2 IP real:", result, flush=True)

    relay()
    for name, alpn in [("web.atenea.one", True), (None, True), ("relay.atenea.one", False)]:
        payload = hello(ca, name)
        if not alpn:
            outgoing = ssl.MemoryBIO()
            connection = ssl.create_default_context().wrap_bio(ssl.MemoryBIO(), outgoing,
                                                              server_hostname=name)
            try:
                connection.do_handshake()
            except ssl.SSLWantReadError:
                pass
            payload = outgoing.read()
        with socket.create_connection((host, 443), timeout=3) as sock:
            sock.sendall(payload)
            assert sock.recv(64) == b"WEB\n"
        print(f"OK WEB: SNI={name!r}, ALPN={alpn}", flush=True)
    errors = []
    started = threading.Event()

    def slow():
        try:
            with socket.create_connection((host, 443), timeout=8) as sock:
                payload = hello(ca, "web.atenea.one")
                sock.sendall(payload[:8])
                started.set()
                for part in (payload[8:16], payload[16:24], payload[24:]):
                    time.sleep(0.7)
                    sock.sendall(part)
                assert sock.recv(64) == b"WEB\n"
        except Exception as error:
            errors.append(error)
            started.set()

    thread = threading.Thread(target=slow)
    thread.start()
    assert started.wait(3)
    before = time.monotonic()
    relay()
    elapsed = time.monotonic() - before
    assert thread.is_alive(), "El cliente lento ya acabó: control de concurrencia inválido"
    assert elapsed < 2, elapsed
    thread.join()
    assert not errors, errors
    print(f"OK ClientHello lento concurrente; otro cliente: {elapsed:.3f}s", flush=True)


def serve(port, cert, key):
    ctx = ssl.SSLContext(ssl.PROTOCOL_TLS_SERVER)
    ctx.minimum_version = ssl.TLSVersion.TLSv1_3
    ctx.set_alpn_protocols(["atenea-relay/2"])
    ctx.load_cert_chain(cert, key)

    class Relay(socketserver.BaseRequestHandler):
        def handle(self):
            self.request.settimeout(6)
            header = exact(self.request, 16)
            assert header[:12] == SIGNATURE and header[12] == 0x21
            family, length = header[13], struct.unpack("!H", header[14:])[0]
            assert family in (0x11, 0x21) and length <= 512
            addresses = exact(self.request, length)
            size = 4 if family == 0x11 else 16
            assert length >= size * 2 + 4
            source = str(ipaddress.ip_address(addresses[:size]))
            with ctx.wrap_socket(self.request, server_side=True) as tls:
                data = tls.recv(64)
                tls.sendall(json.dumps({"source": source, "echo": data.decode()}).encode())

    socketserver.ThreadingTCPServer.allow_reuse_address = True
    with socketserver.ThreadingTCPServer(("127.0.0.1", port), Relay) as server:
        print("READY", flush=True)
        server.serve_forever()


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("mode", choices=["serve", "check"])
    parser.add_argument("args", nargs="+")
    options = parser.parse_args()
    if options.mode == "serve":
        serve(int(options.args[0]), *options.args[1:])
    else:
        check(*options.args)
