#!/usr/bin/env python3
# Copyright 2026 Tutitoos
# SPDX-License-Identifier: Apache-2.0
"""Ejercita el control plane y el lector real del agente; solo laboratorio desechable."""
import json
import os
from pathlib import Path
import subprocess
import sys
import time
import urllib.request
import uuid

sys.path.insert(0, '/src/tests/network-lab')
import cp

P = Path('/lab')
BIN = '/cache/target/release/'
def cli(*args):
    return subprocess.check_output([BIN + 'atenea', *args], text=True).strip()
def api(method, path, data=None):
    status, body = cp.call(method, path, json.dumps(data).encode() if data is not None else None)
    assert status < 400, (status, body)
    return json.loads(body) if body else None

def wait_health():
    for _ in range(120):
        try:
            urllib.request.urlopen('http://localhost:8080/healthz', timeout=2).read()
            return
        except Exception:
            time.sleep(.25)
    raise RuntimeError('control plane no responde')

def probe():
    wait_health()
    cfg = P / 'device.cfg'
    cfg.unlink(missing_ok=True)
    subprocess.run([BIN+'atenea', 'up', '--config', '/lab/agent.toml', '--log', '/lab/agent.log'], check=True, stdout=subprocess.DEVNULL)
    try:
        for _ in range(120):
            try:
                status = json.loads(cli('status', '--socket', '/lab/agent.sock'))
                if status['config']['lease_active']:
                    assert cfg.exists()
                    return
            except (subprocess.CalledProcessError, KeyError, json.JSONDecodeError):
                pass
            time.sleep(.25)
        raise RuntimeError('dispositivo enrolado no recibe configuración')
    finally:
        cli('down', '--socket', '/lab/agent.sock')

if sys.argv[1] == 'setup':
    wait_health()
    cp.login()
    org = api('POST', '/api/v1/orgs', {'name':'restore-lab'})['id']
    root = cli('root-init', '--out', '/lab/offline-root.key')
    net = api('POST', f'/api/v1/orgs/{org}/networks', {'name':'restore-net','ipv4_cidr':'10.77.0.0/24','ipv6_cidr':'fd77::/64','root_anchor':root})['id']
    cli('issuer-cert','--root','/lab/offline-root.key','--issuer-key','/lab/keys/issuer.key','--network-id',net.replace('-',''),'--serial','1','--valid-days','30','--out','/lab/issuer.cert')
    status, body = cp.call('PUT',f'/api/v1/networks/{net}/issuer-cert',P.joinpath('issuer.cert').read_bytes(),'application/octet-stream')
    assert status < 400, body
    info = cli('device-init','--dir','/lab/device')
    device = str(uuid.UUID(dict(line.split('=',1) for line in info.splitlines() if '=' in line)['device_id']))
    token = api('POST',f'/api/v1/networks/{net}/enrollment-tokens',{'ttl_secs':600})
    cli('enroll','--url','http://localhost:8080','--token',token['token'],'--dir','/lab/device','--name','restored-device')
    api('POST',f'/api/v1/networks/{net}/devices/{device}/approve')
    cli('agent-config','--join',json.dumps(token['join']),'--dir','/lab/device','--out','/lab/agent.toml','--state-dir','/lab/agent-state','--config-file','/lab/device.cfg','--ipc-socket','/lab/agent.sock')
    P.joinpath('network').write_text(net)
    P.joinpath('device-id').write_text(device)
    P.joinpath('root-public').write_text(root)
    P.joinpath('offline-root.key').unlink()  # No entra en la copia ni permanece en el nodo de laboratorio.
    probe()
elif sys.argv[1] == 'probe':
    probe()
elif sys.argv[1] == 'key':
    spec = f'''network_id = "{P.joinpath('network').read_text().replace('-', '')}"
device_bundle = "/lab/device/bundle.bin"
validity_secs = 300
tunnel_addrs = ["10.77.0.1", "fd77::1"]
network_prefixes = ["10.77.0.0/24", "fd77::/64"]
'''
    P.joinpath('spec.toml').write_text(spec)
    cli('issue','--issuer-key','/lab/keys/issuer.key','--issuer-cert','/lab/issuer.cert','--spec','/lab/spec.toml','--out','/lab/recovered.cfg')
    cfg = f'''interface = "atenea0"
listen = "0.0.0.0:51820"
ipc_socket = "/lab/key.sock"
noise_key_file = "/lab/device/noise.key"
identity_key_file = "/lab/device/identity.key"
config_file = "/lab/recovered.cfg"
state_dir = "/lab/key-state"
network_id = "{P.joinpath('network').read_text().replace('-', '')}"
device_id = "{P.joinpath('device-id').read_text().replace('-', '')}"
trust_anchor = "{P.joinpath('root-public').read_text()}"
'''
    P.joinpath('key-agent.toml').write_text(cfg)
    cli('up','--config','/lab/key-agent.toml','--log','/lab/key-agent.log')
    try:
        for _ in range(80):
            try:
                if json.loads(cli('status','--socket','/lab/key.sock'))['config']['lease_active']:
                    break
            except subprocess.CalledProcessError:
                pass
            time.sleep(.25)
        else:
            raise RuntimeError('agente rechaza emisor restaurado')
    finally:
        cli('down','--socket','/lab/key.sock')
