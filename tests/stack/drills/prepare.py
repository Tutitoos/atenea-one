"""Material efímero independiente para el segundo relay; CA del ensayo."""
from pathlib import Path
import os
import subprocess

root = Path('/runtime')
# Backchannel privado del ensayo integrado existente.
p = root/'secrets/dex.yaml'
p.chmod(0o600)
p.write_text(p.read_text().replace('issuer: https://panel.stack.test/dex', 'issuer: http://dex:5556/dex'))
p.chmod(0o400)
subprocess.run(['openssl', 'req', '-new', '-newkey', 'ed25519', '-nodes',
    '-keyout', str(root/'secrets/relay2.key'), '-out', str(root/'offline/relay2.csr'),
    '-subj', '/CN=relay2'], check=True)
(root/'offline/relay2.ext').write_text('basicConstraints=critical,CA:FALSE\nkeyUsage=critical,digitalSignature\nextendedKeyUsage=serverAuth\nsubjectAltName=DNS:relay2\n')
subprocess.run(['openssl', 'x509', '-req', '-in', str(root/'offline/relay2.csr'),
    '-CA', str(root/'certs/ca.pem'), '-CAkey', str(root/'offline/ca.key'), '-set_serial', '2',
    '-days', '2', '-extfile', str(root/'offline/relay2.ext'), '-out', str(root/'certs/relay2.pem')], check=True)
p = root/'secrets/relay2.key'
p.chmod(0o400)
os.chown(p, 65532, 65532)
p = root/'config/relay2.toml'
p.write_text((root/'config/relay.toml').read_text().replace('127.0.0.1:8443', '0.0.0.0:8443')
    .replace('/run/secrets/relay.key', '/run/secrets/relay2.key').replace('proxy_protocol = true', 'proxy_protocol = false'))
p.chmod(0o444)
