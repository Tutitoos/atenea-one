#!/usr/bin/env python3
# Copyright 2026 Tutitoos
# SPDX-License-Identifier: Apache-2.0
"""Generación local en contenedor; no se reutiliza ningún secreto de ejemplo."""
import os
from pathlib import Path
import secrets
import subprocess
import sys

root = Path('/runtime')
panel, relay = os.environ['PANEL_HOST'], os.environ['RELAY_HOST']
# Los nombres se interpolan en YAML/TOML y certificados, nunca admiten sintaxis.
assert all(name and all(c.isalnum() or c in '.-' for c in name) for name in (panel, relay))
# Laboratorio por defecto; un piloto fija validez, correo de Dex y nombre del relay por entorno.
days_ca, days_cert = os.environ.get('ATENEA_CA_DAYS', '2'), os.environ.get('ATENEA_CERT_DAYS', '2')
dex_email = os.environ.get('ATENEA_DEX_EMAIL', 'admin@example.test')
relay_name = os.environ.get('ATENEA_RELAY_NAME', 'lab')
assert days_ca.isdigit() and days_cert.isdigit() and 0 < int(days_cert) <= int(days_ca)
assert dex_email.count('@') == 1 and all(c.isalnum() or c in '@.-_+' for c in dex_email)
assert relay_name and all(c.isalnum() or c in '.-_' for c in relay_name)

def save(name, value, mode=0o400, uid=65532):
    path = root / name
    path.write_text(value)
    path.chmod(mode)
    os.chown(path, uid, uid)

if len(sys.argv) == 1:
    for directory in ('secrets', 'config', 'certs', 'offline'):
        (root / directory).mkdir(mode=0o755 if directory in ('config', 'certs') else 0o700)
    # Docker Compose monta cada fichero, nunca el directorio privado completo.
    passwords = {key: secrets.token_hex(32) for key in ('postgres', 'app', 'migrator', 'oidc', 'dex')}
    save('secrets/postgres-password', passwords['postgres'], uid=999)
    save('secrets/app-database-url', f"postgres://atenea_app:{passwords['app']}@postgres/atenea")
    save('secrets/migration-database-url', f"postgres://atenea_migrator:{passwords['migrator']}@postgres/atenea")
    save('secrets/oidc-client-secret', passwords['oidc'])
    save('offline/dex-password', passwords['dex'], uid=0)
    save('secrets/issuer.key', secrets.token_hex(32))
    save('secrets/relay-signing.hex', secrets.token_hex(32))
    relay_id = secrets.token_hex(16)
    assert relay_id != '0' * 32
    save('config/relay-id', relay_id, mode=0o444)
    save('secrets/postgres-init.sql', f"CREATE ROLE atenea_migrator LOGIN PASSWORD '{passwords['migrator']}';\nCREATE ROLE atenea_app LOGIN PASSWORD '{passwords['app']}';\nALTER DATABASE atenea OWNER TO atenea_migrator;\n\\connect atenea\nALTER SCHEMA public OWNER TO atenea_migrator;\n", uid=999)
    save('secrets/dex.yaml', f'''issuer: https://{panel}/dex
storage:
  type: memory
web:
  http: 0.0.0.0:5556
  allowedOrigins: ['https://{panel}']
oauth2:
  skipApprovalScreen: true
enablePasswordDB: true
staticClients:
  - id: atenea-control-plane
    name: Atenea One
    secret: {passwords['oidc']}
    redirectURIs: ['https://{panel}/auth/callback']
staticPasswords:
  - email: {dex_email}
    hash: BCRYPT_RUNTIME
    username: admin
    userID: 08a8684b-db88-4b73-90a9-3cd1661f5466
''')
    subprocess.run(['openssl', 'req', '-x509', '-newkey', 'ed25519', '-nodes', '-days', days_ca, '-keyout', str(root/'offline/ca.key'), '-out', str(root/'certs/ca.pem'), '-subj', '/CN=Atenea Stack CA', '-addext', 'basicConstraints=critical,CA:TRUE', '-addext', 'keyUsage=critical,keyCertSign,cRLSign'], check=True)
    subprocess.run(['openssl', 'req', '-new', '-newkey', 'ed25519', '-nodes', '-keyout', str(root/'secrets/relay.key'), '-out', str(root/'offline/relay.csr'), '-subj', f'/CN={relay}'], check=True)
    save('offline/relay.ext', f'basicConstraints=critical,CA:FALSE\nkeyUsage=critical,digitalSignature\nextendedKeyUsage=serverAuth\nsubjectAltName=DNS:{relay},DNS:{panel}\n', uid=0)
    subprocess.run(['openssl', 'x509', '-req', '-in', str(root/'offline/relay.csr'), '-CA', str(root/'certs/ca.pem'), '-CAkey', str(root/'offline/ca.key'), '-set_serial', '1', '-days', days_cert, '-extfile', str(root/'offline/relay.ext'), '-out', str(root/'certs/relay.pem')], check=True)
    (root/'secrets/relay.key').chmod(0o400)
    os.chown(root/'secrets/relay.key', 65532, 65532)
else:
    anchor = (root/'config/root.pub').read_text().strip()
    assert len(anchor) == 64 and len(os.environ['RELAY_PUB']) == 64
    relay_id = (root/'config/relay-id').read_text().strip()
    assert len(relay_id) == 32 and all(c in '0123456789abcdef' for c in relay_id) and int(relay_id, 16) != 0
    save('config/coordinator.toml', f'''listen = "0.0.0.0:8081"
stun_bind = "0.0.0.0"
stun_ports = [3478, 3479]
anchors = ["{anchor}"]
[[relays]]
relay_id = "{relay_id}"
name = "{relay_name}"
udp = "{relay}:443"
tls = "{relay}:443"
tls_name = "{relay}"
''', mode=0o444)
    save('config/relay.toml', f'''relay_id = "{relay_id}"
udp_listen = "0.0.0.0:443"
tcp_listen = "127.0.0.1:8443"
coordinator_keys = ["{os.environ['RELAY_PUB']}"]
tls_cert_file = "/etc/atenea/relay.pem"
tls_key_file = "/run/secrets/relay.key"
status_socket = "/tmp/relay.sock"
proxy_protocol = true
''', mode=0o444)
