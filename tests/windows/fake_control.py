# Copyright 2026 Tutitoos
# SPDX-License-Identifier: Apache-2.0
"""Control plane mínimo y SINTÉTICO para `atenea network join/leave` en el runner de CI.

Acepta el enrolamiento (201) y la revocación (200) sin validar firmas ni tokens, y apunta cada petición en un
fichero para que la prueba compruebe lo que llegó. No es un servidor real: no emite configuraciones.
Uso: python fake_control.py <puerto> <network_id_hex32> <registro>
"""
import http.server
import json
import sys

port, network, log = int(sys.argv[1]), sys.argv[2], sys.argv[3]
DEVICE = '11111111-2222-4333-8444-555555555555'


class Handler(http.server.BaseHTTPRequestHandler):
    def reply(self, status, body):
        data = json.dumps(body).encode()
        self.send_response(status)
        self.send_header('Content-Type', 'application/json')
        self.send_header('Content-Length', str(len(data)))
        self.end_headers()
        self.wfile.write(data)

    def do_POST(self):
        self.rfile.read(int(self.headers.get('Content-Length') or 0))
        with open(log, 'a', encoding='utf-8') as out:
            out.write(f'POST {self.path}\n')
        if self.path == '/api/v1/enroll':
            self.reply(201, {'device_id': DEVICE, 'network_id': network, 'status': 'approved'})
        elif self.path == '/api/v1/device/leave':
            self.reply(200, {})
        else:
            self.reply(404, {'error': {'code': 'not_found'}})

    def do_GET(self):
        # El agente sincroniza en segundo plano: sin configuración que servir.
        self.reply(404, {'error': {'code': 'not_found'}})

    def log_message(self, *args):
        pass


http.server.ThreadingHTTPServer(('127.0.0.1', port), Handler).serve_forever()
