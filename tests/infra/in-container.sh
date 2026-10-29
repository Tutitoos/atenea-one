#!/usr/bin/env bash
# Copyright 2026 Tutitoos
# SPDX-License-Identifier: Apache-2.0
set -euo pipefail
umask 077
mkdir -p /lab
cp -a /src/infra /lab/
cd /lab/infra/ansible
# Todo el material privado vive en el filesystem desechable del nodo.
age-keygen -o /lab/identity.age
export SOPS_AGE_KEY_FILE=/lab/identity.age
recipient=$(age-keygen -y "$SOPS_AGE_KEY_FILE")
ssh-keygen -q -t ed25519 -N '' -f /lab/admin
mkdir /runtime
export PANEL_HOST=panel.stack.test RELAY_HOST=relay.stack.test
python3 /src/deploy/compose/prod/bootstrap.py
image() { while read -r service digest; do [[ $service != "$1" ]] || { echo "$digest"; return; }; done < /tmp/infra-images; return 1; }
password=$(cat /runtime/offline/dex-password)
hash=$(docker run --rm --pull never --network none "$(image caddy)" caddy hash-password --plaintext "$password")
export DEX_HASH=$hash
unset password hash
python3 - <<'PY'
import os
from pathlib import Path
p = Path('/runtime/secrets/dex.yaml')
p.chmod(0o600)
p.write_text(p.read_text().replace('BCRYPT_RUNTIME', os.environ['DEX_HASH']))
p.chmod(0o400)
PY
unset DEX_HASH
docker run --rm --pull never --network none --user 0 -v /runtime:/runtime "$(image tools)" root-init --out /runtime/offline/root.key > /runtime/config/root.pub
RELAY_PUB=$(docker run --rm --pull never --network none --user 0 -v /runtime:/runtime:ro -e ATENEA_COORD_RELAY_SIGNING_KEY_FILE=/runtime/secrets/relay-signing.hex --entrypoint /usr/local/bin/atenea-coordinator "$(image tools)" --print-relay-key)
export RELAY_PUB
python3 /src/deploy/compose/prod/bootstrap.py config
python3 - <<'PY'
import json, shutil, yaml
from pathlib import Path
images = dict(line.split() for line in Path('/tmp/infra-images').read_text().splitlines())
images['netns'] = images['haproxy']
Path('/lab/plain.yaml').write_text(json.dumps({'files': {p.name: p.read_text() for p in Path('/runtime/secrets').iterdir()}}))
public = Path('/lab/public')
public.mkdir()
for name in ['config', 'certs']:
 shutil.copytree(Path('/runtime')/name, public/name)
vars = {'atenea_node_role': 'principal', 'atenea_admin_user': 'operador',
 'atenea_admin_public_key': Path('/lab/admin.pub').read_text().strip(),
 'atenea_ssh_sources': ['192.0.2.0/24', '2001:db8::/32'],
 'atenea_apt_update_cache': False,
 'atenea_extra_udp_ports': [51820],
 'atenea_secrets_source': '/lab/secrets.sops.yaml', 'atenea_config_source': str(public),
 'atenea_compose_source': '/src/deploy/compose/prod',
 'atenea_stack_images': {k: v for k,v in images.items() if k != 'tools'},
 'atenea_env': {'ATENEA_RUNTIME_DIR': '/opt/atenea', 'PANEL_HOST': 'panel.stack.test',
                'RELAY_HOST': 'relay.stack.test', 'CADDY_TLS': 'internal'}}
inv = {'all': {'children': {'principal': {'hosts': {'localhost': {'ansible_connection': 'local'}}}, 'relays': {'hosts': {}}}, 'vars': vars}}
Path('/lab/inventory.yml').write_text(yaml.safe_dump(inv))
PY
sops encrypt --age "$recipient" --input-type yaml --output-type yaml /lab/plain.yaml > /lab/secrets.sops.yaml
printf 'creation_rules:\n  - path_regex: .*[.]sops[.]yaml$\n    age: %s\n' "$recipient" > /lab/rotation.yaml
bash /lab/infra/secrets/rotate.sh /lab/secrets.sops.yaml /lab/rotation.yaml
sops decrypt --output-type json /lab/secrets.sops.yaml > /lab/rotated.json
python3 - <<'PY'
import json
assert json.load(open('/lab/plain.yaml')) == json.load(open('/lab/rotated.json'))
PY
for playbook in site relay upgrade; do ansible-playbook -i /lab/inventory.yml "$playbook.yml" --syntax-check; done
ansible-lint --offline --profile production -p site.yml relay.yml upgrade.yml
yamllint -c /src/tests/infra/yamllint.yml /lab/infra/ansible /lab/infra/secrets
start=$SECONDS
ansible-playbook -i /lab/inventory.yml site.yml
echo "PASS convergencia real en $((SECONDS-start)) s"
ansible-playbook -i /lab/inventory.yml site.yml | tee /lab/second.log
python3 /src/tests/infra/check.py principal /lab/second.log
# Ansible reemplaza los ficheros con un inodo nuevo: el contenedor en ejecución debe verlos tras converger.
coord=$(docker compose --project-directory /opt/atenea ps -q coordinator)
seen() { cat "/proc/$(docker inspect -f '{{.State.Pid}}' "$1")/root/etc/atenea/coordinator.toml"; }
started=$(docker inspect -f '{{.State.StartedAt}}' "$coord")
if seen "$coord" | grep -q regresion-fichero-reemplazado; then echo 'ERROR: marca previa inesperada' >&2; exit 1; fi
printf '# regresion-fichero-reemplazado\n' >> /lab/public/config/coordinator.toml
ansible-playbook -i /lab/inventory.yml site.yml
coord=$(docker compose --project-directory /opt/atenea ps -q coordinator)
seen "$coord" | grep -q regresion-fichero-reemplazado
test "$(docker inspect -f '{{.State.StartedAt}}' "$coord")" != "$started"
python3 /src/tests/infra/check.py principal
echo 'PASS fichero de configuración reemplazado visible en el contenedor tras recrearlo'
python3 /src/tests/infra/restart-docker.py
# CA interna copiada desde Caddy: verificación TLS, nunca --insecure.
caddy=$(docker compose --project-directory /opt/atenea ps -q caddy)
docker cp "$caddy:/data/caddy/pki/authorities/local/root.crt" /lab/caddy-root.crt
curl --fail --silent --show-error --noproxy '*' --cacert /lab/caddy-root.crt --resolve panel.stack.test:443:127.0.0.1 https://panel.stack.test/ > /lab/panel.html
test -s /lab/panel.html
echo 'PASS panel HTTPS tras HAProxy/Caddy con CA interna'
# Nuevo digest real de la misma aplicación: solo metadatos de imagen, sin compilación/red.
docker create --pull never --network none --name infra-next-build "$(image coordinator)" >/dev/null
docker commit --change 'LABEL atenea.infra.upgrade=next' infra-next-build atenea-infra-next >/dev/null
docker rm infra-next-build >/dev/null
next=$(docker image inspect atenea-infra-next --format '{{.Id}}')
test "$next" != "$(image coordinator)"
python3 - "$next" <<'PY'
import sys, yaml
from pathlib import Path
inv = yaml.safe_load(Path('/lab/inventory.yml').read_text())
inv['all']['vars']['atenea_stack_images']['coordinator'] = sys.argv[1]
inv['all']['vars']['atenea_backup_verified'] = True
Path('/lab/upgrade.yml').write_text(yaml.safe_dump(inv))
PY
ansible-playbook -i /lab/upgrade.yml upgrade.yml
python3 /src/tests/infra/check.py principal
cp /opt/atenea/compose.override.yaml /lab/previous-map
cp /opt/atenea/.env /lab/previous-env
# Caddy existe localmente pero no puede arrancar como coordinator; falla salud, no pull.
python3 - "$(image caddy)" <<'PY'
import sys, yaml
from pathlib import Path
inv = yaml.safe_load(Path('/lab/upgrade.yml').read_text())
inv['all']['vars']['atenea_stack_images']['coordinator'] = sys.argv[1]
inv['all']['vars']['atenea_stack_health_timeout'] = 25
Path('/lab/failure.yml').write_text(yaml.safe_dump(inv))
PY
if ansible-playbook -i /lab/failure.yml upgrade.yml | tee /lab/failure.log; then
 echo 'ERROR: upgrade forzado debía fallar' >&2; exit 1
fi
python3 - <<'PY'
from pathlib import Path
text = Path('/lab/failure.log').read_text()
assert 'rollback al mapa de digests anterior saludable' in text, text
assert 'El stack no alcanzó salud' in text, text
PY
cmp /lab/previous-map /opt/atenea/compose.override.yaml
cmp /lab/previous-env /opt/atenea/.env
python3 /src/tests/infra/check.py principal
echo 'PASS upgrade feliz y upgrade fallido con rollback al mapa anterior'
if [ -f /tmp/infra-next-images ]; then
  # Ensayo entre dos revisiones reales (N = imágenes con que convergió el nodo; N+1 = /tmp/infra-next-images).
  # 1) baseline N, 2) testigos, 3) upgrade que falla DESPUÉS de migrar 0008 y rescue a N, 4) reintento correcto.
  next_image() { while read -r service digest; do [[ $service != "$1" ]] || { echo "$digest"; return; }; done < /tmp/infra-next-images; return 1; }
  stack() { docker compose --project-directory /opt/atenea "$@"; }
  running() { docker inspect -f '{{.Image}}' "$(stack ps -q "$1")"; }
  sql() { stack exec -T postgres psql -U postgres -d atenea -At "$@"; }
  versions() { sql -c 'SELECT string_agg(version::text, '"','"' ORDER BY version) FROM _sqlx_migrations WHERE success'; }
  fingerprint() { sql -c "SELECT md5(string_agg(id::text||oidc_subject||email, ',' ORDER BY id)) FROM users"; }
  roles() { sql -c "SELECT count(*) FROM information_schema.columns WHERE table_name IN ('devices','enrollment_tokens') AND column_name='role'"; }
  expect() { [ "$2" = "$3" ] || { echo "ERROR $1: esperado '$3', obtenido '$2'" >&2; exit 1; }; echo "PASS $1"; }
  commit_of() { docker exec "$(stack ps -q "$1")" "/usr/local/bin/atenea-$1" --version; }
  inventory() { # fichero, control-plane, coordinator, relay
    python3 - "$@" <<'PY'
import sys, yaml
from pathlib import Path
inv = yaml.safe_load(Path('/lab/inventory.yml').read_text())
v = inv['all']['vars']
v['atenea_stack_images'].update({'control-plane': sys.argv[2], 'coordinator': sys.argv[3], 'relay': sys.argv[4]})
v['atenea_backup_verified'] = True
v['atenea_stack_health_timeout'] = 40
Path(sys.argv[1]).write_text(yaml.safe_dump(inv))
PY
  }
  ansible-playbook -i /lab/inventory.yml site.yml
  for s in control-plane coordinator relay; do expect "N: $s con la imagen de la revisión anterior" "$(running "$s")" "$(image "$s")"; done
  expect 'N: historial de migraciones 1–7' "$(versions)" '1,2,3,4,5,6,7'
  expect 'N: columnas role aún no existen' "$(roles)" 0
  if [ -n "${INFRA_N_COMMIT:-}" ]; then
    expect 'N: control-plane informa el commit anterior' "$(commit_of control-plane | sed 's/.*(\(.*\))/\1/')" "$INFRA_N_COMMIT"
  fi
  sql -c "INSERT INTO users(id,oidc_issuer,oidc_subject,email,display_name) VALUES ('00000000-0000-0000-0000-000000000001','https://rehearsal.invalid','owner','owner@example.invalid','Propietario sintético'),('00000000-0000-0000-0000-000000000007','https://rehearsal.invalid','member','member@example.invalid','Miembro sintético')" >/dev/null
  witness=$(fingerprint)
  [ -n "$witness" ] && echo 'PASS testigos de datos sembrados en N'
  cp /opt/atenea/compose.override.yaml /lab/rehearsal-previous-map
  cp /opt/atenea/.env /lab/rehearsal-previous-env
  # Fallo tras migrar: control-plane N+1 migra y arranca; el coordinator es Caddy y no llega a healthy.
  inventory /lab/rehearsal-fail.yml "$(next_image control-plane)" "$(image caddy)" "$(image relay)"
  if ansible-playbook -i /lab/rehearsal-fail.yml upgrade.yml | tee /lab/rehearsal-fail.log; then
    echo 'ERROR: el upgrade forzado debía fallar' >&2; exit 1
  fi
  python3 - <<'PY'
from pathlib import Path
text = Path('/lab/rehearsal-fail.log').read_text()
assert 'rollback al mapa de digests anterior saludable' in text, text
assert 'El stack no alcanzó salud' in text, text
PY
  echo 'PASS el upgrade forzado falló y ejecutó el rescue'
  cmp /lab/rehearsal-previous-map /opt/atenea/compose.override.yaml
  cmp /lab/rehearsal-previous-env /opt/atenea/.env
  echo 'PASS rescue restauró mapa de digests y entorno byte a byte'
  python3 /src/tests/infra/check.py principal
  for s in control-plane coordinator relay; do expect "rescue: $s vuelve a la imagen de la revisión anterior" "$(running "$s")" "$(image "$s")"; done
  expect 'rescue: 0008 sigue aplicada (el rescue no deshace migraciones)' "$(versions)" '1,2,3,4,5,6,7,8'
  expect 'rescue: columnas role presentes bajo el binario anterior' "$(roles)" 2
  expect 'rescue: datos sembrados intactos' "$(fingerprint)" "$witness"
  if [ -n "${INFRA_N_COMMIT:-}" ]; then
    expect 'rescue: control-plane en ejecución es el de la revisión anterior' "$(commit_of control-plane | sed 's/.*(\(.*\))/\1/')" "$INFRA_N_COMMIT"
  fi
  # Reintento correcto con 0008 ya aplicada (la migración debe ser idempotente).
  inventory /lab/rehearsal-next.yml "$(next_image control-plane)" "$(next_image coordinator)" "$(next_image relay)"
  ansible-playbook -i /lab/rehearsal-next.yml upgrade.yml
  python3 /src/tests/infra/check.py principal
  for s in control-plane coordinator relay; do expect "N+1: $s con la imagen nueva" "$(running "$s")" "$(next_image "$s")"; done
  expect 'N+1: historial 1–8' "$(versions)" '1,2,3,4,5,6,7,8'
  expect 'N+1: datos sembrados intactos' "$(fingerprint)" "$witness"
  if [ -n "${INFRA_NEXT_COMMIT:-}" ]; then
    for s in control-plane coordinator relay; do expect "N+1: $s informa el commit nuevo" "$(commit_of "$s" | sed 's/.*(\(.*\))/\1/')" "$INFRA_NEXT_COMMIT"; done
  fi
  curl --fail --silent --show-error --noproxy '*' --cacert /lab/caddy-root.crt --resolve panel.stack.test:443:127.0.0.1 https://panel.stack.test/ >/dev/null
  echo 'PASS N+1: panel HTTPS tras el upgrade'
  echo 'PASS ensayo N -> fallo tras migrar 0008 -> rescue a N -> N+1 con coordinator y relay reales'
fi
systemctl disable --now atenea-stack.service
docker compose --project-directory /opt/atenea down -v
# Mismo nodo desechable, sin representar otro proveedor; reutiliza solo certificado/clave relay.
python3 - <<'PY'
import json, secrets, yaml
from pathlib import Path
inv = yaml.safe_load(Path('/lab/inventory.yml').read_text())
inv['all']['children'] = {'relays': {'hosts': {'localhost': {'ansible_connection': 'local'}}}}
v = inv['all']['vars']
v.update(atenea_node_role='relay', atenea_stack_dir='/opt/atenea-relay', atenea_secrets_source='/lab/relay.sops.yaml')
v.pop('atenea_extra_udp_ports')  # el relay comprueba el comportamiento por defecto (solo 443 y 3478)
v['atenea_env']['ATENEA_RUNTIME_DIR'] = '/opt/atenea-relay'
v['atenea_stack_images'] = {'relay': v['atenea_stack_images']['relay'], 'stun': v['atenea_stack_images']['coordinator']}
p = Path('/lab/public/config/relay.toml')
primary_id = Path('/runtime/config/relay-id').read_text().strip()
secondary_id = secrets.token_hex(16)
assert secondary_id != primary_id and int(secondary_id, 16) != 0
p.write_text(p.read_text().replace(f'relay_id = "{primary_id}"', f'relay_id = "{secondary_id}"').replace('127.0.0.1:8443', '0.0.0.0:443').replace('proxy_protocol = true', 'proxy_protocol = false'))
Path('/lab/public/config/relay-id').write_text(secondary_id)
Path('/lab/public/config/stun.toml').write_text('listen = "127.0.0.1:8081"\nstun_bind = "0.0.0.0"\nstun_ports = [3478, 3479]\nanchors = ["' + Path('/runtime/config/root.pub').read_text().strip() + '"]\n')
Path('/lab/relay-inventory.yml').write_text(yaml.safe_dump(inv))
Path('/lab/relay-plain.yaml').write_text(json.dumps({'files': {'relay.key': Path('/runtime/secrets/relay.key').read_text()}}))
PY
sops encrypt --age "$recipient" --input-type yaml --output-type yaml /lab/relay-plain.yaml > /lab/relay.sops.yaml
ansible-playbook -i /lab/relay-inventory.yml relay.yml
ansible-playbook -i /lab/relay-inventory.yml relay.yml | tee /lab/relay-second.log
python3 /src/tests/infra/check.py relay /lab/relay-second.log
cd /lab/infra/tofu
tofu fmt -check -recursive
cd examples/local
tofu init -input=false
tofu validate
tofu plan -input=false -out=/lab/local.plan
printf '\nINFRA OK: stack real, migraciones, HTTPS, digests, upgrade/rollback, relay+STUN, secretos e idempotencia.\n'
