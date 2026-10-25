#!/usr/bin/env python3
"""Aserciones sobre Docker y nftables reales del nodo desechable."""
import json
from pathlib import Path
import re
import subprocess
import sys

role = sys.argv[1]
relay = role == 'relay'
root = Path('/opt/atenea-relay' if relay else '/opt/atenea')
if len(sys.argv) == 3:
    text = Path(sys.argv[2]).read_text()
    assert re.search(r'localhost\s+:.*changed=0\s+unreachable=0\s+failed=0', text), text

base = ['docker', 'compose', '--project-directory', str(root)]
ids = subprocess.check_output(base + ['ps', '--all', '-q'], text=True).split()
containers = json.loads(subprocess.check_output(['docker', 'inspect', *ids], text=True))
expected = {'relay', 'stun'} if relay else {'haproxy', 'caddy', 'postgres', 'dex', 'control-plane', 'coordinator', 'relay'}
assert {c['Config']['Labels']['com.docker.compose.service'] for c in containers} == expected
assert all(c['State'].get('Health', {}).get('Status') == 'healthy' for c in containers)
assert len(containers) == len(expected)
config = json.loads(subprocess.check_output(base + ['config', '--format', 'json'], text=True))
for container in containers:
    service = container['Config']['Labels']['com.docker.compose.service']
    reference = config['services'][service]['image']
    digest = subprocess.check_output(['docker', 'image', 'inspect', reference, '--format', '{{.Id}}'], text=True).strip()
    assert container['Image'] == digest, (service, container['Image'], digest)
ports = set()
for container in containers:
    for key, bindings in container['NetworkSettings']['Ports'].items():
        for binding in bindings or []:
            ports.add((key.split('/')[1], int(binding['HostPort'])))
assert ports == ({('tcp', 443), ('udp', 443), ('udp', 3478)} if relay else {('tcp', 80), ('tcp', 443), ('udp', 443), ('udp', 3478)}), ports
rules = json.loads(subprocess.check_output(['nft', '-j', 'list', 'table', 'inet', 'atenea_host'], text=True))['nftables']
chains = [r['chain'] for r in rules if 'chain' in r]
assert all(c['policy'] == 'drop' for c in chains if c['name'] in ['input', 'forward'])
allowed = {'tcp': set(), 'udp': set()}
for entry in rules:
    for expr in entry.get('rule', {}).get('expr', []):
        match = expr.get('match', {})
        payload = match.get('left', {}).get('payload', {})
        if payload.get('field') == 'dport':
            value = match['right']
            allowed[payload['protocol']].update(value['set'] if isinstance(value, dict) else [value])
assert allowed == {'tcp': {22, 443} if relay else {22, 80, 443}, 'udp': {443, 3478}}, allowed
for secret in (root / 'secrets').iterdir():
    stat = secret.stat()
    assert stat.st_mode & 0o777 == 0o400, secret
    assert stat.st_uid == stat.st_gid == (999 if secret.name in ['postgres-password', 'postgres-init.sql'] else 65532), secret
for directory in ['config', 'certs']:
    assert (root / directory).stat().st_mode & 0o777 == 0o755, directory
    for public in (root / directory).iterdir():
        assert public.stat().st_mode & 0o777 == 0o644, public
if relay:
    assert {p.name for p in (root / 'secrets').iterdir()} == {'relay.key'}
text = subprocess.check_output(['sshd', '-T'], text=True)
for setting in ['permitrootlogin no', 'passwordauthentication no', 'kbdinteractiveauthentication no', 'allowtcpforwarding no']:
    assert setting in text, setting
print(f'PASS {role}: {len(expected)} healthy; docker ps/inspect y nftables puertos exactos; secretos 0400/UID; SSH; idempotencia si se solicita')
