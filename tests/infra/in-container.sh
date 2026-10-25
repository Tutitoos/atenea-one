#!/usr/bin/env bash
set -euo pipefail
mkdir -p /lab
cp -a /src/infra /lab/
cd /lab/infra/ansible
# Claves desechables: ninguna sale de este contenedor.
umask 077
age-keygen -o /lab/identity.age 2>/lab/age-public
export SOPS_AGE_KEY_FILE=/lab/identity.age
recipient=$(age-keygen -y "$SOPS_AGE_KEY_FILE")
ssh-keygen -q -t ed25519 -N '' -f /lab/admin
python3 - <<'PY'
import json, secrets
from pathlib import Path
names = ['issuer.key', 'relay-signing.hex', 'postgres-password', 'postgres-init.sql',
         'app-database-url', 'migration-database-url', 'oidc-client-secret', 'relay.key', 'dex.yaml']
# Solo se comprueban montaje y permisos: estos valores no arrancan aplicaciones.
Path('/lab/plain.yaml').write_text(json.dumps({'files': {name: secrets.token_hex(32) for name in names}}))
PY
sops encrypt --age "$recipient" --input-type yaml --output-type yaml /lab/plain.yaml > /lab/secrets.sops.yaml
sops decrypt /lab/secrets.sops.yaml > /lab/roundtrip.yaml
python3 - <<'PY'
import yaml
assert yaml.safe_load(open('/lab/plain.yaml')) == yaml.safe_load(open('/lab/roundtrip.yaml'))
PY
printf 'creation_rules:\n  - path_regex: .*[.]sops[.]yaml$\n    age: %s\n' "$recipient" > /lab/rotation.yaml
bash /lab/infra/secrets/rotate.sh /lab/secrets.sops.yaml /lab/rotation.yaml
sops decrypt /lab/secrets.sops.yaml > /lab/rotated.yaml
python3 - <<'PY'
import yaml
assert yaml.safe_load(open('/lab/plain.yaml')) == yaml.safe_load(open('/lab/rotated.yaml'))
PY
# No ejecuta las imágenes de producción: valida el Compose real si ya existe.
# Las imágenes Atenea aún no están publicadas por digest; dry-run no hace pull/up.
python3 - <<'PY'
import yaml
from pathlib import Path
digest = 'debian@sha256:4c25275446ca371766b8b093c696782eabce06263fccacb8d5d813b3dc44b308'
env = {'ATENEA_RUNTIME_DIR': '/opt/atenea', 'PANEL_HOST': 'panel.example.test',
       'RELAY_HOST': 'relay.example.test', 'CADDY_TLS': 'internal',
       'CONTROL_PLANE_IMAGE': digest, 'COORDINATOR_IMAGE': digest, 'RELAY_IMAGE': digest}
source = '/src/deploy/compose/prod'
if not Path(source + '/compose.yaml').exists():
 source = '/lab/compose'
 Path(source).mkdir()
 Path(source + '/compose.yaml').write_text(yaml.safe_dump({'services': {'test': {'image': digest}}}))
 print('AVISO: Compose mínimo; falta integrar StackImages')
else:
 print('Compose producción presente: validación estructural dry-run, imágenes de ensayo; NO arranque')
inventory = {'all': {'children': {'principal': {'hosts': {'localhost': {'ansible_connection': 'local'}}}, 'relays': {'hosts': {}}}, 'vars': {
 'atenea_node_role': 'principal', 'atenea_admin_user': 'operador',
 'atenea_admin_public_key': Path('/lab/admin.pub').read_text().strip(),
 'atenea_ssh_sources': ['192.0.2.0/24', '2001:db8::/32'],
 'atenea_secrets_source': '/lab/secrets.sops.yaml', 'atenea_stack_mode': 'dry-run',
 'atenea_compose_source': source, 'atenea_env': env}}}
Path('/lab/inventory.yml').write_text(yaml.safe_dump(inventory))
PY
for playbook in site relay upgrade; do ansible-playbook -i /lab/inventory.yml "$playbook.yml" --syntax-check; done
ansible-lint --offline -p site.yml relay.yml upgrade.yml
# yamllint coincide con Ansible: documentos YAML, no plantillas Jinja.
yamllint -c /src/tests/infra/yamllint.yml /lab/infra/ansible /lab/infra/secrets
ansible-playbook -i /lab/inventory.yml site.yml
ansible-playbook -i /lab/inventory.yml site.yml | tee /lab/second.log
python3 - <<'PY'
import re
text = open('/lab/second.log').read()
assert re.search(r'localhost\s+:.*changed=0\s+unreachable=0\s+failed=0', text), text
PY
nft -j list table inet atenea_host > /lab/firewall.json
sshd -T > /lab/sshd.txt
python3 - <<'PY'
import json, os
rules=json.load(open('/lab/firewall.json'))['nftables']
chains=[r['chain'] for r in rules if 'chain' in r]
assert all(c['policy']=='drop' for c in chains if c['name'] in ['input','forward'])
text=open('/lab/sshd.txt').read()
for expected in ['permitrootlogin no', 'passwordauthentication no', 'kbdinteractiveauthentication no', 'allowtcpforwarding no']:
 assert expected in text, expected
from pathlib import Path
for secret in Path('/opt/atenea/secrets').iterdir():
 assert secret.stat().st_mode & 0o777 == 0o400, secret
ports = {'tcp': set(), 'udp': set()}
for entry in rules:
 for expr in entry.get('rule', {}).get('expr', []):
  match = expr.get('match', {})
  payload = match.get('left', {}).get('payload', {})
  if payload.get('field') == 'dport':
   value = match['right']
   ports[payload['protocol']].update(value['set'] if isinstance(value, dict) else [value])
assert ports == {'tcp': {22, 80, 443}, 'udp': {443, 3478}}, ports
print('SSH, nftables deny-by-default, secretos 0400 e idempotencia: OK')
PY
docker info --format '{{.ServerVersion}}'
docker compose version
# Ensayar también el rol secundario en el mismo laboratorio: no es otra máquina real.
python3 - <<'PY'
import json, yaml, secrets
from pathlib import Path
inventory = yaml.safe_load(Path('/lab/inventory.yml').read_text())
inventory['all']['children'] = {'relays': {'hosts': {'localhost': {'ansible_connection': 'local'}}}}
variables = inventory['all']['vars']
variables.update(atenea_node_role='relay', atenea_stack_dir='/opt/atenea-relay', atenea_secrets_source='/lab/relay.sops.yaml')
variables['atenea_env']['ATENEA_RUNTIME_DIR'] = '/opt/atenea-relay'
Path('/lab/relay-inventory.yml').write_text(yaml.safe_dump(inventory))
Path('/lab/relay-plain.yaml').write_text(json.dumps({'files': {'relay.key': secrets.token_hex(32)}}))
PY
sops encrypt --age "$recipient" --input-type yaml --output-type yaml /lab/relay-plain.yaml > /lab/relay.sops.yaml
ansible-playbook -i /lab/relay-inventory.yml relay.yml
ansible-playbook -i /lab/relay-inventory.yml relay.yml | tee /lab/relay-second.log
nft -j list table inet atenea_host > /lab/relay-firewall.json
python3 - <<'PY'
import json, re
from pathlib import Path
assert re.search(r'localhost\s+:.*changed=0\s+unreachable=0\s+failed=0', Path('/lab/relay-second.log').read_text())
ports = {'tcp': set(), 'udp': set()}
for entry in json.load(open('/lab/relay-firewall.json'))['nftables']:
 for expr in entry.get('rule', {}).get('expr', []):
  match = expr.get('match', {})
  payload = match.get('left', {}).get('payload', {})
  if payload.get('field') == 'dport':
   value = match['right']
   ports[payload['protocol']].update(value['set'] if isinstance(value, dict) else [value])
assert ports == {'tcp': {22, 443}, 'udp': {443, 3478}}, ports
assert {p.name for p in Path('/opt/atenea-relay/secrets').iterdir()} == {'relay.key'}
assert Path('/opt/atenea-relay/secrets/relay.key').stat().st_mode & 0o777 == 0o400
print('Relay secundario: puertos limitados, sin secretos de firma, segunda convergencia changed=0: OK')
PY
cd /lab/infra/tofu
tofu fmt -check -recursive
cd examples/local
tofu init -input=false
tofu validate
tofu plan -input=false -out=/lab/local.plan
printf '\nINFRA OK: SOPS/age real, lint, base+Docker real, Compose dry-run; imágenes de aplicación NO arrancadas.\n'
