#!/usr/bin/env python3
"""OIDC por código real, cookies/CSRF y agentes reales; navegador en tests/e2e-panel."""
import http.cookiejar
from html.parser import HTMLParser
import json
from pathlib import Path
import ssl
import subprocess
import time
import urllib.error
import urllib.parse
import urllib.request

BASE = 'https://panel.stack.test'
jar = http.cookiejar.CookieJar()
# El backchannel de Dex es HTTP privado; el cliente siempre visita su proxy HTTPS.
class FrontendRedirect(urllib.request.HTTPRedirectHandler):
    def redirect_request(self, req, fp, code, msg, headers, newurl):
        return super().redirect_request(req, fp, code, msg, headers,
            newurl.replace('http://dex:5556', BASE))

# La CA es efímera y exclusiva de esta ejecución; no se desactiva la verificación TLS.
opener = urllib.request.build_opener(urllib.request.ProxyHandler({}), FrontendRedirect(), urllib.request.HTTPCookieProcessor(jar),
    urllib.request.HTTPSHandler(context=ssl.create_default_context(cafile='/runtime/certs/ca.pem')))
csrf = None

def request(path, body=None, method=None, content_type='application/json'):
    headers = {}
    if csrf:
        headers['X-CSRF-Token'] = csrf
    if body is not None:
        headers['Content-Type'] = content_type
        if not isinstance(body, bytes):
            body = json.dumps(body).encode()
    with opener.open(urllib.request.Request(urllib.parse.urljoin(BASE, path).replace('http://dex:5556', BASE), data=body,
            headers=headers, method=method), timeout=15) as response:
        data = response.read()
        return response.geturl(), data

def api(path, body=None, method=None):
    _, data = request(path, body, method)
    return json.loads(data) if data else None

def lab(*args, check=True):
    result = subprocess.run(['bash', '/src/tests/stack/lab.sh', *args],
        text=True, capture_output=True, timeout=180)
    if check and result.returncode:
        raise RuntimeError(f'lab {args[0]}: {result.stderr}')
    return result.stdout.strip() if check else result.returncode == 0

class LoginForm(HTMLParser):
    def __init__(self):
        super().__init__()
        self.action = None
        self.fields = {}
    def handle_starttag(self, tag, attrs):
        attrs = dict(attrs)
        if tag == 'form':
            self.action = attrs.get('action', '')
        if tag == 'input' and attrs.get('name'):
            self.fields[attrs['name']] = attrs.get('value', '')

started = time.monotonic()
url, html = request('/auth/login')
form = LoginForm()
form.feed(html.decode())
assert form.action is not None and 'login' in form.fields and 'password' in form.fields
assert url.startswith(BASE + '/dex/'), 'Dex debe permanecer bajo HTTPS'
form.fields.update(login='admin@example.test', password=Path('/runtime/offline/dex-password').read_text().strip())
url, _ = request(urllib.parse.urljoin(url, form.action), urllib.parse.urlencode(form.fields).encode(),
    content_type='application/x-www-form-urlencoded')
session = api('/api/v1/session')
assert session['user']['email'] == 'admin@example.test'
csrf = session['csrf_token']
assert any(c.name == 'atenea_session' and c.secure for c in jar)
print('PASS OIDC Dex por código sobre HTTPS, cookie Secure y sesión real', flush=True)
# La misma escritura sin CSRF debe fallar antes de crear datos.
saved, csrf = csrf, None
try:
    api('/api/v1/orgs', {'name': 'sin-csrf'})
    raise AssertionError('Se aceptó una escritura sin CSRF')
except urllib.error.HTTPError as error:
    assert error.code == 403
csrf = saved
org = api('/api/v1/orgs', {'name': 'stack-lab'})
network = api(f"/api/v1/orgs/{org['id']}/networks", {'name': 'red-stack',
    'ipv4_cidr': '10.77.0.0/24', 'ipv6_cidr': 'fd77::/64',
    'root_anchor': Path('/tmp/lab/root.pub').read_text().strip()})
net = '/api/v1/networks/' + network['id']
lab('issuer-cert', network['id'])
request(net + '/issuer-cert', Path('/tmp/lab/issuer.cert').read_bytes(), 'PUT', 'application/octet-stream')
joins = {}
for name in ('a', 'b'):
    info = lab('device-init', name)
    enrollment = api(net + '/enrollment-tokens', {'ttl_secs': 3600})
    joins[name] = json.dumps(enrollment['join'])
    lab('enroll', name, enrollment['join']['url'], enrollment['token'])
    device = next(d for d in api(net + '/devices') if d['name'] == 'host-' + name)
    assert device['status'] == 'pending' and ('fingerprint=' + device['fingerprint']) in info
    assert api(net + '/devices/' + device['id'] + '/approve', b'')['status'] == 'approved'
print('PASS CSRF rechazado sin token; red, certificado y dos agentes CLI enrolados/aprobados con huellas', flush=True)
api(net + '/policies', {'src': {'kind': 'any'}, 'dst': {'kind': 'any'}, 'proto': 0, 'port_min': 0, 'port_max': 0})
for name in ('a', 'b'):
    lab('start-agent', name, joins[name])
devices = api(net + '/devices')
ip_b = next(d['ipv4'] for d in devices if d['name'] == 'host-b')

def field_or_none(agent, dotted):
    # Mientras el agente no ha cargado su configuración la lista de pares está vacía.
    try:
        return lab('field', agent, dotted)
    except RuntimeError:
        return None

def wait_path(path):
    deadline = time.monotonic() + 180
    while time.monotonic() < deadline:
        lab('ping', 'a', ip_b, check=False)
        if path == field_or_none('a', 'peers.0.path') and path == field_or_none('b', 'peers.0.path') and lab('ping', 'a', ip_b, check=False):
            return
        time.sleep(3)
    raise AssertionError(f'No se alcanzó {path}: ' + str(field_or_none('a', 'peers.0.path')))

# Carga sostenida: conserva alta/aprobación y agentes, sin ejecutar la revocación E2E.
if __import__('os').environ.get('ATENEA_STACK_LOAD') == 'soak':
    import os
    import runpy
    import sys
    os.environ['ATENEA_LOAD_PEER_IP'] = ip_b
    sys.argv = ['traffic.py', 'soak', os.environ['ATENEA_SOAK_HOURS']]
    runpy.run_path('/src/tests/load/traffic.py', run_name='__main__')
    raise SystemExit(0)

# El simulacro reutiliza el alta real sin repetir el recorrido destructivo de revocación.
if __import__('os').environ.get('ATENEA_STACK_DRILLS') == '1':
    import runpy
    runpy.run_path('/src/tests/stack/drills/scenarios.py', init_globals=globals())
    raise SystemExit(0)

for block, path in (('none', 'direct'), ('p2p', 'relay-udp'), ('udp', 'relay-tls')):
    lab('block', block)
    wait_path(path)
    assert lab('transfer', ip_b) == 'ok'
    print(f'PASS {path}: TCP 1000000 bytes con SHA-256 coincidente' +
        (' por HAProxy TLS443/SNI verificado' if path == 'relay-tls' else ''), flush=True)
lab('block', 'none')
device_b = next(d for d in devices if d['name'] == 'host-b')
api(net + '/devices/' + device_b['id'] + '/revoke', b'')
deadline = time.monotonic() + 150
while lab('ping', 'a', ip_b, check=False):
    assert time.monotonic() < deadline, 'Revocación no efectiva'
    time.sleep(2)
print(f'PASS revocación efectiva; recorrido API/agentes en {time.monotonic()-started:.1f} s', flush=True)
print('STACK E2E: TODO OK (API OIDC; navegador cubierto por tests/e2e-panel)', flush=True)
