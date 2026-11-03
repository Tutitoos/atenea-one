#!/usr/bin/env python3
# Copyright 2026 Tutitoos
# SPDX-License-Identifier: Apache-2.0
"""Router de rutas del laboratorio: `/v1/*` al coordinator y todo lo demás al control plane.

Hace el papel del Caddy del stack real (una sola URL pública para el control plane y el coordinator), que es lo
que permite que `[coordinator].url` sea la URL del propio código de unión. Uso: path-proxy.py PUERTO CP COORD
(CP y COORD como host:puerto). Solo para pruebas; no es un proxy de producción.
"""
import http.client
import sys
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer

HOP = {'connection', 'keep-alive', 'transfer-encoding', 'content-length', 'host', 'upgrade', 'te', 'trailer'}
PORT, CP, COORD = int(sys.argv[1]), sys.argv[2], sys.argv[3]


class Handler(BaseHTTPRequestHandler):
    protocol_version = 'HTTP/1.1'

    def forward(self):
        target = COORD if self.path.startswith('/v1/') else CP
        host, port = target.rsplit(':', 1)
        body = self.rfile.read(int(self.headers.get('Content-Length') or 0))
        headers = {k: v for k, v in self.headers.items() if k.lower() not in HOP}
        conn = http.client.HTTPConnection(host, int(port), timeout=45)
        try:
            conn.request(self.command, self.path, body, headers)
            reply = conn.getresponse()
            data = reply.read()
        except OSError:
            self.send_error(502)
            return
        finally:
            conn.close()
        self.send_response(reply.status)
        for key, value in reply.getheaders():
            if key.lower() not in HOP:
                self.send_header(key, value)
        self.send_header('Content-Length', str(len(data)))
        self.end_headers()
        self.wfile.write(data)

    do_GET = do_POST = do_PUT = do_PATCH = do_DELETE = forward

    def log_message(self, *_):
        pass


class Server(ThreadingHTTPServer):
    def handle_error(self, *_):  # un agente que corta el long-poll no es un error del router
        pass


Server(('0.0.0.0', PORT), Handler).serve_forever()
