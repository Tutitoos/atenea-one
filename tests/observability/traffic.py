#!/usr/bin/env python3
# Copyright 2026 Tutitoos
# SPDX-License-Identifier: Apache-2.0
"""Tráfico real; reutiliza el cliente OIDC y el laboratorio de fases 5/6."""
import base64
import http.server
import importlib.util
import json
import os
import pathlib
import subprocess
import threading
import time
import urllib.error
import urllib.request
import uuid

os.environ['CP_URL'] = 'http://198.51.100.254:8080'
spec = importlib.util.spec_from_file_location('cp', 'tests/network-lab/cp.py')
cp = importlib.util.module_from_spec(spec)
spec.loader.exec_module(cp)
LAB = pathlib.Path('/tmp/lab')
private = ['PRIVATE_TOKEN_CANARY', 'PRIVATE_COOKIE_CANARY', 'PRIVATE_KEY_CANARY', 'private-canary@example.test', '203.0.113.247', 'PRIVATE_CONFIG_CANARY']

def cmd(*args):
    return subprocess.check_output(args, text=True).strip()

def api(method, path, body=None, binary=None):
    status, text = cp.call(method, path, binary if binary is not None else (json.dumps(body).encode() if body is not None else None), 'application/octet-stream' if binary is not None else 'application/json')
    assert status < 400, (status, text)
    return json.loads(text) if text else None

# Las métricas de inet se encaminan solo dentro de esta red efímera de pruebas.
def proxy(port):
    class Handler(http.server.BaseHTTPRequestHandler):
        def do_GET(self):
            if self.path != '/metrics':
                self.send_error(404)
                return
            body = urllib.request.urlopen(f'http://198.51.100.10:{port}/metrics', timeout=3).read()
            self.send_response(200)
            self.send_header('Content-Type', 'text/plain; version=0.0.4')
            self.end_headers()
            self.wfile.write(body)
        def log_message(self, *_):
            pass
    server = http.server.ThreadingHTTPServer(('0.0.0.0', port), Handler)
    threading.Thread(target=server.serve_forever, daemon=True).start()

proxy(9102)
proxy(9103)
cp.login()
private += [c.value for c in cp.opener()[0]]
private += ['admin@example.test', '198.51.100.10', '198.51.100.254']
private += [(LAB/'issuer.key').read_text().strip(), (LAB/'relay-signing.hex').read_text().strip()]
org = api('POST', '/api/v1/orgs', {'name': 'observabilidad'})['id']
net = api('POST', f'/api/v1/orgs/{org}/networks', {'name':'observabilidad', 'ipv4_cidr':'10.77.0.0/24', 'ipv6_cidr':'fd77::/64', 'root_anchor':(LAB/'root.pub').read_text().strip()})['id']
cmd('bash', 'tests/e2e-panel/lab.sh', 'issuer-cert', net)
api('PUT', f'/api/v1/networks/{net}/issuer-cert', binary=(LAB/'issuer.cert').read_bytes())
for name in ['a', 'b']:
    info = cmd('bash', 'tests/e2e-panel/lab.sh', 'device-init', name)
    device = str(uuid.UUID(next(line.split('=',1)[1] for line in info.splitlines() if line.startswith('device_id='))))
    token = api('POST', f'/api/v1/networks/{net}/enrollment-tokens', {'ttl_secs':600})
    private.append(token['token'])
    cmd('bash', 'tests/e2e-panel/lab.sh', 'enroll', name, cp.BASE, token['token'])
    api('POST', f'/api/v1/networks/{net}/devices/{device}/approve')
    (LAB/f'{name}.join.json').write_text(json.dumps(token['join']))
api('POST', f'/api/v1/networks/{net}/policies', {'src':{'kind':'any'}, 'dst':{'kind':'any'}, 'proto':1})
for name in ['a','b']:
    cmd('bash', 'tests/e2e-panel/lab.sh', 'start-agent', name, (LAB/f'{name}.join.json').read_text())
cmd('bash', 'tests/e2e-panel/lab.sh', 'block', 'p2p')
for attempt in range(80):
    addresses = cmd('ip', '-n', 'b', '-j', '-4', 'addr', 'show').strip()
    ips = [entry['local'] for interface in json.loads(addresses) if interface['ifname']=='atenea0' for entry in interface['addr_info']]
    if ips:
        subprocess.run(['ip','netns','exec','a','ping','-c','1','-W','1',ips[0]], stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
    metrics = urllib.request.urlopen('http://198.51.100.10:9103/metrics').read().decode()
    if any(line.startswith('atenea_relay_packets_total{') and float(line.rsplit(' ',1)[1]) > 0 for line in metrics.splitlines()):
        break
    time.sleep(1)
else:
    raise AssertionError('el relay no recibió tráfico autenticado')

# Cabeceras, query, cuerpo, ruta y UDP con marcadores privados.
body = json.dumps({'token':private[0], 'cookie':private[1], 'key':private[2], 'email':private[3], 'ip':private[4], 'config':private[5]}).encode()
for origin, path in [(cp.BASE,'/api/v1/enroll'), ('http://198.51.100.10:8081','/v1/announce'), ('http://198.51.100.10:8081','/v1/relay')]:
    request = urllib.request.Request(origin+path+'?token='+private[0], data=body, headers={'Authorization':'Bearer '+private[0], 'Cookie':private[1], 'Content-Type':'application/json'})
    try:
        urllib.request.urlopen(request, timeout=3).read()
    except urllib.error.HTTPError:
        pass
import socket
with socket.socket(socket.AF_INET,socket.SOCK_DGRAM) as udp:
    udp.sendto(body, ('198.51.100.10',443))
# El listener público jamás sirve la representación Prometheus.
for origin in [cp.BASE, 'http://198.51.100.10:8081']:
    try:
        response = urllib.request.urlopen(origin+'/metrics', timeout=3)
        assert b'atenea_http_requests_total' not in response.read()
    except urllib.error.HTTPError as error:
        assert error.code == 404
# También se ejercita el arranque erróneo: Figment antes imprimía entradas privadas.
for binary, prefix, field in [('atenea-control-plane','ATENEA_CP_','CONFIG_VALIDITY_SECS'), ('atenea-coordinator','ATENEA_COORD_','PRESENCE_TTL_SECS'), ('atenea-relay','ATENEA_RELAY_','UDP_LISTEN')]:
    env = dict(os.environ)
    env[prefix+field] = private[0]
    process = subprocess.run([binary], env=env, capture_output=True, text=True)
    assert process.returncode != 0
    assert private[0] not in process.stdout+process.stderr
import ssl
context = ssl.create_default_context(cafile=str(LAB/'certs/ca.pem'))
private += [base64.b64encode((LAB/'a.cfg').read_bytes()).decode()]
context.set_alpn_protocols(['atenea-relay/2'])
with socket.create_connection(('198.51.100.10',443), timeout=3) as connection:
    with context.wrap_socket(connection, server_hostname='relay.atenea.one') as tls:
        tls.sendall(b'GET /metrics HTTP/1.1\r\nHost: relay.atenea.one\r\n\r\n')
        try:
            assert b'atenea_' not in tls.recv(4096)
        except (ssl.SSLError, ConnectionResetError):
            pass

# Se examinan los bytes originales, NO un filtro posterior de registros.
for service, filename in [('control-plane','cp.log'),('coordinator','coord.log'),('relay','relay.log')]:
    text = (LAB/filename).read_text()
    assert text, f'sin registro real para {service}'
    for secret in private:
        assert secret not in text, f'fuga privada en {service}'
    directory = pathlib.Path('/obs/logs')/service
    directory.mkdir(parents=True,exist_ok=True)
    (directory/f'{service}-json.log').write_text(''.join(json.dumps({'log':line+'\n','stream':'stdout','time':'2026-10-25T00:00:00Z','attrs':{'com.docker.compose.service':service}})+'\n' for line in text.splitlines()))
(LAB/'private.json').write_text(json.dumps(private))
(LAB/'observability.ready').touch()
print('Tráfico enrolamiento/announce/relay y privacidad: OK', flush=True)
while True:
    time.sleep(60)
