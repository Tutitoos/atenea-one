#!/usr/bin/env python3
# Copyright 2026 Tutitoos
# SPDX-License-Identifier: Apache-2.0
"""Tráfico TCP sintético: respuesta idéntica y vigilancia sin reconexiones."""
import socket
import sys
import time

mode, address = sys.argv[1:3]
family = socket.AF_INET6 if ":" in address else socket.AF_INET
if mode == "serve":
    with socket.socket(family, socket.SOCK_STREAM) as listener:
        listener.setsockopt(socket.SOL_SOCKET, socket.SO_REUSEADDR, 1)
        listener.bind((address, 9000))
        listener.listen()
        while True:
            connection, _ = listener.accept()
            with connection:
                try:
                    while data := connection.recv(4096):
                        connection.sendall(data)
                except ConnectionError:
                    # El vigilante termina cerrando su conexión; el servidor sigue disponible.
                    pass
else:
    assert mode in ("probe", "watch"), mode
    with socket.socket(family, socket.SOCK_STREAM) as connection:
        connection.settimeout(4)
        connection.connect((address, 9000))
        sequence = 0
        while True:
            payload = f"atenea-sintetico-{sequence:08d}".encode()
            connection.sendall(payload)
            received = b""
            while len(received) < len(payload):
                chunk = connection.recv(len(payload) - len(received))
                assert chunk, "conexión cerrada"
                received += chunk
            assert received == payload, (received, payload)
            sequence += 1
            print(sequence, flush=True)
            if mode == "probe":
                break
            time.sleep(0.05)
