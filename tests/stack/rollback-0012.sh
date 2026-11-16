#!/usr/bin/env bash
# Copyright 2026 Tutitoos
# SPDX-License-Identifier: Apache-2.0
# Retroceso v0.1.31 (binario desplegado en el piloto, migraciones 1–11) sobre 0012 (devices.versions/versions_at)
# con escrituras nuevas. No ejecuta down migrations: el binario anterior debe funcionar con el esquema ampliado.
#
# A diferencia de rollback-0008/0009/0010, no necesita las imágenes de laboratorio: compila los dos control plane
# en el anfitrión (perfil dev, un solo CARGO_TARGET_DIR) y usa solo PostgreSQL 17 en Docker, con datos sintéticos.
# Requiere docker, cargo, python3 y un openssl con Ed25519 (`pkeyutl -rawin`); ATENEA_PREVIOUS_TAG cambia N.
set -euo pipefail
ROOT=$(git -C "$(dirname "$0")" rev-parse --show-toplevel)
cd "$ROOT"
umask 077
START=$SECONDS
RUN="atenea-rollback-0012-$$"
PREVIOUS_TAG=${ATENEA_PREVIOUS_TAG:-v0.1.31}
TMP=$(mktemp -d "${TMPDIR:-/tmp}/atenea-rollback-0012.XXXXXX")
TMP=$(cd "$TMP" && pwd -P)
PG=$RUN-pg
PORT=${ATENEA_ROLLBACK_PG_PORT:-54172}
CP_PORT=${ATENEA_ROLLBACK_CP_PORT:-54173}
TARGET=${CARGO_TARGET_DIR:-$ROOT/target}
POSTGRES_IMAGE=postgres:17@sha256:b8faeff7712663c2773a190181e1246e37e364b887611c04658c28b44e2408fe
CP_PID=
cleanup() {
  local rc=$?
  trap - EXIT
  local passed=0
  if [ -f "$TMP/checks" ]; then passed=$(wc -l < "$TMP/checks" | tr -d " "); fi
  printf "%s comprobaciones; pasan %s; fallan %s\n" "$((passed + (rc != 0)))" "$passed" "$((rc != 0))"
  if [ -n "$CP_PID" ]; then kill "$CP_PID" >/dev/null 2>&1 || true; fi
  docker rm -fv "$PG" >/dev/null 2>&1 || true
  if [ -d "$TMP/previous" ]; then git worktree remove --force "$TMP/previous"; fi
  rm -rf "$TMP"
  exit "$rc"
}
trap cleanup EXIT
trap 'exit 130' INT
trap 'exit 143' TERM

docker info >/dev/null
PREVIOUS=$(git rev-parse "$PREVIOUS_TAG^{commit}")
git worktree add --detach "$TMP/previous" "$PREVIOUS"
set -- "$TMP/previous"/apps/control-plane/migrations/*.sql
[ "$#" -eq 11 ]
test -f "$TMP/previous/apps/control-plane/migrations/0011_configuration_versions_not_after.sql"
set -- apps/control-plane/migrations/*.sql
[ "$#" -eq 12 ]
test -f apps/control-plane/migrations/0012_device_versions.sql
printf 'N tag %s commit=%s; N+1 working-tree HEAD=%s\n' "$PREVIOUS_TAG" "$PREVIOUS" "$(git rev-parse HEAD)"

# Binarios: cada revisión se compila con su propio árbol; Cargo distingue las rutas de los paquetes locales.
mkdir -p "$TMP/bin"
build() {
  (cd "$1" && CARGO_TARGET_DIR=$TARGET nice -n 19 cargo build --locked -j 2 -p atenea-control-plane "${@:3}")
  cp "$TARGET/debug/atenea-control-plane" "$2"
}
build "$TMP/previous" "$TMP/bin/n"
build "$ROOT" "$TMP/bin/n1" -p atenea-cli
cp "$TARGET/debug/atenea" "$TMP/bin/atenea"
cmp -s "$TMP/bin/n" "$TMP/bin/n1" && { echo 'N y N+1 son el mismo binario' >&2; exit 1; }
printf 'N sha256=%s\nN+1 sha256=%s\n' "$(shasum -a 256 "$TMP/bin/n" | cut -c1-64)" "$(shasum -a 256 "$TMP/bin/n1" | cut -c1-64)"

# Secretos exclusivos de esta ejecución, nunca fixtures ni salida.
rnd() { od -An -N32 -tx1 /dev/urandom | tr -d ' \n'; }
PG_PASSWORD=$(rnd); APP_PASSWORD=$(rnd); SESSION_TOKEN=$(rnd); CSRF_TOKEN=$(rnd); ISSUER_SEED=$(rnd)
docker run -d --name "$PG" -p "127.0.0.1:$PORT:5432" \
  -e "POSTGRES_PASSWORD=$PG_PASSWORD" -e POSTGRES_DB=atenea "$POSTGRES_IMAGE" >/dev/null
for ((i=0; i<60; i++)); do
  if docker exec "$PG" pg_isready -U postgres -d atenea >/dev/null 2>&1; then break; fi
  sleep 1
done
docker exec "$PG" pg_isready -U postgres -d atenea >/dev/null
printf '%s\n' "$ISSUER_SEED" > "$TMP/issuer.key"

sql() { docker exec -i "$PG" psql -X -U postgres -d atenea -v ON_ERROR_STOP=1 -At "$@"; }
cp_vars=(ATENEA_CP_ISSUER_KEY_FILE="$TMP/issuer.key" ATENEA_CP_LISTEN="127.0.0.1:$CP_PORT"
  ATENEA_CP_PUBLIC_URL=https://control.example.invalid ATENEA_CP_APPLICATION_ROLE=atenea_app
  RUST_LOG=atenea_control_plane=info)
migrate() { env "${cp_vars[@]}" ATENEA_CP_DATABASE_URL="postgres://postgres:$PG_PASSWORD@127.0.0.1:$PORT/atenea" "$1" --migrate; }
# `env` ejecuta el binario con exec: $! es su PID y stop lo termina de verdad (un puerto ocupado por un N antiguo falsearía la prueba).
start() {
  env "${cp_vars[@]}" ATENEA_CP_DATABASE_URL="postgres://atenea_app:$APP_PASSWORD@127.0.0.1:$PORT/atenea" "$1" > "$TMP/cp.log" 2>&1 &
  CP_PID=$!
  sleep 1
  kill -0 "$CP_PID"
}
stop() { kill "$CP_PID"; wait "$CP_PID" 2>/dev/null || true; CP_PID=; }
check_versions() {
  local actual
  actual=$(sql -c 'SELECT string_agg(version::text,'"'"','"'"' ORDER BY version) FROM _sqlx_migrations WHERE success')
  [ "$actual" = "$1" ] || { printf 'Migraciones inesperadas: %s\n' "$actual" >&2; return 1; }
}
ok() { printf 'PASS %s\n' "$1"; printf '.\n' >> "$TMP/checks"; }
expect_sql() { [ "$(sql -c "$1")" = "$2" ]; ok "$3"; }
checksums() { sql -c "SELECT string_agg(version::text||':'||encode(checksum,'hex'),',' ORDER BY version) FROM _sqlx_migrations WHERE version<=$1"; }
# Columnas nuevas por dispositivo (nombre estable; excluye el dispositivo que crea N en el retroceso).
versions_state() { sql -c "SELECT string_agg(name||':'||versions::text||':'||coalesce(versions_at::text,'null'),',' ORDER BY name) FROM devices WHERE name<>'lab-rollback'"; }

printf '\nFASE 1: N %s, migraciones 1–11 y siembra\n' "$PREVIOUS_TAG"
migrate "$TMP/bin/n"
check_versions 1,2,3,4,5,6,7,8,9,10,11
ok 'N: historial 1–11 exitoso'
HISTORY=$(checksums 11)
sql -v app_password="$APP_PASSWORD" -v session="$SESSION_TOKEN" -v csrf="$CSRF_TOKEN" <<'SQL'
ALTER ROLE atenea_app PASSWORD :'app_password';
INSERT INTO users(id,oidc_issuer,oidc_subject,email,display_name) VALUES
('00000000-0000-0000-0000-000000000001','https://rollback.invalid','owner','owner@example.invalid','Propietario sintético');
INSERT INTO sessions(id,user_id,csrf_hash,csrf_token,expires_at) VALUES (sha256(:'session'::bytea),'00000000-0000-0000-0000-000000000001',sha256(:'csrf'::bytea),:'csrf',now()+interval '8 hours');
SQL
cat > "$TMP/probe.py" <<'PY'
import hashlib, json, os, pathlib, subprocess, sys, time, urllib.error, urllib.request
stage = sys.argv[1]
p = pathlib.Path(os.environ['PROBE_DIR'])
state_path = p/'state.json'
s = json.loads(state_path.read_text()) if state_path.exists() else {}
base = os.environ['PROBE_BASE']
def ck(value, label):
    if not value:
        raise AssertionError(label)
    with (p/'checks').open('a') as out: out.write('.\n')
    print('PASS '+stage+': '+label, flush=True)
def cli(*args):
    return subprocess.check_output([os.environ['PROBE_CLI'], *args], stderr=subprocess.PIPE).decode().strip()
def req(path, body=None, method=None, device=None, raw=False):
    method = method or ('POST' if body is not None else 'GET')
    data = body if isinstance(body, bytes) else (json.dumps(body).encode() if body is not None else b'')
    headers = {'Cookie':'atenea_session='+os.environ['UPGRADE_SESSION'], 'X-CSRF-Token':os.environ['UPGRADE_CSRF'], 'Content-Type':'application/json'}
    if raw: headers['Content-Type'] = 'application/octet-stream'
    if device:
        ts, nonce = str(int(time.time())), os.urandom(16).hex()
        key = bytes.fromhex((p/device/'identity.key').read_text().strip())
        (p/'key.der').write_bytes(bytes.fromhex('302e020100300506032b657004220420')+key)
        message = b'atenea-one/api-auth/v1\0'+f'{method}\n{path}\n{ts}\n{nonce}\n'.encode()+hashlib.blake2s(data).digest()
        (p/'message').write_bytes(message)
        sig = subprocess.check_output(['openssl','pkeyutl','-sign','-rawin','-keyform','DER','-inkey',str(p/'key.der'),'-in',str(p/'message')], stderr=subprocess.PIPE).hex()
        headers = {'Authorization':f"AtDevice v1 device={s[device].replace('-','')}, ts={ts}, nonce={nonce}, sig={sig}", 'Content-Type':'application/json'}
    try:
        with urllib.request.urlopen(urllib.request.Request(base+path,data=data if method!='GET' else None,headers=headers,method=method),timeout=10) as r:
            result=r.read()
            return result if raw else (json.loads(result) if result else None)
    except urllib.error.HTTPError as e:
        # Solo el código de error de la API (cuerpo JSON corto), nunca material de sesión.
        raise RuntimeError(f'{stage} {method} {path}: HTTP {e.code} {e.read()[:60]!r}') from None
def report(body, device):
    # El control plane limita los informes a uno cada 5 s por reloj de la base; el laboratorio puede ir más rápido.
    for attempt in range(60):
        try:
            return req('/api/v1/device/status', body, device=device)
        except RuntimeError as e:
            if 'rate_limited' not in str(e) or attempt==59: raise
            time.sleep(1)
for attempt in range(60):
    try:
        req('/healthz'); break
    except Exception:
        if attempt==59: raise
        time.sleep(1)
ck(True, 'healthz responde')
def enroll(name, approve=False):
    cli('device-init','--dir',str(p/name))
    token=req(net+'/enrollment-tokens',{'ttl_secs':3600})
    body={'token':token['token'],'bundle':(p/name/'bundle.bin').read_bytes().hex(),'name':'lab-'+name}
    d=req('/api/v1/enroll',body)
    s[name]=d['device_id']
    ck(d['status']=='pending',name+' inscrito pendiente por API')
    if approve:
        ck(req(net+'/devices/'+s[name]+'/approve',{})['status']=='approved',name+' aprobado')
policy={'src':{'kind':'any'},'dst':{'kind':'any'},'proto':0,'port_min':0,'port_max':0}
svc=[{'port':8443,'proto':'tcp','scheme':'https','label':'web'}]
ver=[{'component':'agent','version':'0.1.32','commit':'0123456789abcdef0123456789abcdef01234567'},{'component':'cli','version':'0.1.32','commit':'unknown'}]
if stage=='N':
    s['org']=req('/api/v1/orgs',{'name':'rollback-org'})['id']
    root=cli('root-init','--out',str(p/'root.key')).splitlines()[-1]
    s['root']=root
    s['net']=req('/api/v1/orgs/'+s['org']+'/networks',{'name':'rollback-net','ipv4_cidr':'10.42.0.0/24','ipv6_cidr':'fd42::/64','root_anchor':root})['id']
net='/api/v1/networks/'+s.get('net','')
org='/api/v1/orgs/'+s.get('org','')
if stage=='N':
    cli('issuer-cert','--root',str(p/'root.key'),'--issuer-key',os.environ['PROBE_ISSUER'],'--network-id',s['net'].replace('-',''),'--serial','1','--valid-days','30','--out',str(p/'issuer.cert'))
    req(net+'/issuer-cert',(p/'issuer.cert').read_bytes(),'PUT',raw=True)
    enroll('approved',approve=True)
    enroll('peer',approve=True)
    enroll('pending')
    s['policy']=req(net+'/policies',policy)['id']
    report({'paths':[],'services':svc},'peer')
    ck(any(d['id']==s['pending'] and d['status']=='pending' for d in req(net+'/devices')),'siembra: dos aprobados, uno pendiente, política y servicios del par')
elif stage=='nuevo':
    devices=req(net+'/devices')
    ck(len(devices)==3 and all(d['versions']==[] and d['versions_at'] is None for d in devices),'N+1: dispositivos migrados con versions [] y versions_at nulo')
    ck(any(d['id']==s['peer'] and d['services']==svc for d in devices),'N+1: los servicios escritos por N siguen en la base')
    ck(all(d['versions']==[] and d['versions_at'] is None for d in req('/api/v1/device/peers',device='approved')),'N+1: peers sin versiones antes de informar')
    report({'paths':[],'versions':ver},'peer')
    ck(any(d['id']==s['peer'] and d['versions']==ver and d['versions_at'] for d in req(net+'/devices')),'N+1: informe firmado guarda versions y versions_at')
    ck(any(d['device_id']==s['peer'] and d['versions']==ver and d['versions_at'] for d in req('/api/v1/device/peers',device='approved')),'N+1: /device/peers de un par con política muestra las versiones')
    report({'paths':[]},'peer')
    ck(any(d['id']==s['peer'] and d['versions']==ver for d in req(net+'/devices')),'N+1: un informe sin versions no borra las guardadas')
elif stage=='retroceso':
    ck(any(o['id']==s['org'] for o in req('/api/v1/orgs')),'organización legible')
    ck(any(n['id']==s['net'] for n in req(org+'/networks')),'lista de redes legible')
    ck(req(net)['name']=='rollback-net','detalle de red legible')
    ck(len(req(net+'/devices'))==3,'dispositivos legibles')
    ck(any(d['id']==s['policy'] for d in req(net+'/policies')),'política conservada')
    ck(isinstance(req(net+'/audit'),list),'auditoría legible')
    ck(any(d['device_id']==s['peer'] for d in req('/api/v1/device/peers',device='approved')),'N: /device/peers firmado lista al par')
    ck(len(req('/api/v1/device/config',device='approved',raw=True))>100,'N emite configuración firmada')
    # Informe de N sin versions: no debe tocar las columnas nuevas (comprobado por SQL fuera del cliente).
    ck(report({'paths':[]},'approved') is None,'N acepta informe firmado de dispositivo aprobado (204)')
    ck(report({'paths':[],'services':svc},'peer') is None,'N acepta el informe del par con servicios (204)')
    s['rollback_org']=req('/api/v1/orgs',{'name':'escritura-retroceso'})['id']
    s['rollback_net']=req('/api/v1/orgs/'+s['rollback_org']+'/networks',{'name':'red-retroceso','ipv4_cidr':'10.44.0.0/24','ipv6_cidr':'fd44::/64','root_anchor':s['root']})['id']
    enroll('rollback',approve=True)
    ck(len(req('/api/v1/device/config',device='rollback',raw=True))>100,'N emite configuración firmada a dispositivo nuevo')
    ck(isinstance(req('/api/v1/device/peers',device='rollback'),list),'N peers firmado de dispositivo nuevo')
    temporary=req(net+'/policies',dict(policy,proto=6,port_min=443,port_max=443))
    req(net+'/policies/'+temporary['id'],method='DELETE')
    ck(not any(d['id']==temporary['id'] for d in req(net+'/policies')),'N crea y borra política')
    ck(any(o['id']==s['rollback_org'] for o in req('/api/v1/orgs')),'N crea organización y red')
elif stage=='reupgrade':
    devices=req(net+'/devices')
    ck(len(devices)==4,'persistencia: cuatro dispositivos')
    ck(any(d['id']==s['peer'] and d['versions']==ver and d['versions_at'] for d in devices),'N+1 vuelve a leer las versiones escritas antes del retroceso')
    ck(all(d['versions']==[] and d['versions_at'] is None for d in devices if d['id']!=s['peer']),'resto de dispositivos sigue con versions [] y versions_at nulo')
    ck(any(o['id']==s['rollback_org'] for o in req('/api/v1/orgs')),'organización escrita por N conservada')
    ck(any(n['id']==s['rollback_net'] for n in req('/api/v1/orgs/'+s['rollback_org']+'/networks')),'red escrita por N conservada')
    report({'paths':[],'versions':[]},'peer')
    ck(any(d['id']==s['peer'] and d['versions']==[] and d['versions_at'] for d in req(net+'/devices')),'N+1: una lista vacía sustituye las versiones y fija versions_at')
state_path.write_text(json.dumps(s))
PY
probe() {
  PROBE_DIR=$TMP PROBE_BASE="http://127.0.0.1:$CP_PORT" PROBE_CLI=$TMP/bin/atenea PROBE_ISSUER=$TMP/issuer.key \
    UPGRADE_SESSION=$SESSION_TOKEN UPGRADE_CSRF=$CSRF_TOKEN python3 "$TMP/probe.py" "$1"
}
start "$TMP/bin/n"
probe N
stop
# Toda la siembra salvo las columnas nuevas; en N no existen y la resta es una no-operación.
cat > "$TMP/fingerprint.sql" <<'SQL'
SELECT md5(string_agg(v,E'\n' ORDER BY v)) FROM (
 SELECT (to_jsonb(t)-'versions'-'versions_at')::text v FROM devices t
 UNION ALL SELECT to_jsonb(t)::text FROM enrollment_tokens t
 UNION ALL SELECT to_jsonb(t)::text FROM users t
 UNION ALL SELECT to_jsonb(t)::text FROM organizations t
 UNION ALL SELECT to_jsonb(t)::text FROM networks t
 UNION ALL SELECT to_jsonb(t)::text FROM access_policies t
 UNION ALL SELECT to_jsonb(t)::text FROM memberships t
) fixture;
SQL
BASELINE=$(sql < "$TMP/fingerprint.sql")
printf '\nFASE 2: upgrade N+1, 0012 y escritura de versiones\n'
migrate "$TMP/bin/n1"
check_versions 1,2,3,4,5,6,7,8,9,10,11,12
ok 'historial 1–12 exitoso'
[ "$HISTORY" = "$(checksums 11)" ]
ok 'checksums 1–11 intactos'
HISTORY12=$(checksums 12)
[ "$BASELINE" = "$(sql < "$TMP/fingerprint.sql")" ]; ok 'valores de toda la siembra intactos al migrar'
expect_sql "SELECT column_default||'|'||is_nullable||'|'||data_type FROM information_schema.columns WHERE table_name='devices' AND column_name='versions'" "'[]'::jsonb|NO|jsonb" 'devices.versions jsonb NOT NULL DEFAULT []'
expect_sql "SELECT is_nullable||'|'||data_type FROM information_schema.columns WHERE table_name='devices' AND column_name='versions_at'" 'YES|timestamp with time zone' 'devices.versions_at timestamptz nulo permitido'
expect_sql "SELECT count(*) FROM devices WHERE versions<>'[]'::jsonb OR versions_at IS NOT NULL" 0 'filas previas con versions [] y versions_at NULL'
start "$TMP/bin/n1"
probe nuevo
stop
VERSIONS_N1=$(versions_state)
expect_sql "SELECT count(*) FROM devices WHERE name='lab-peer' AND jsonb_array_length(versions)=2 AND versions_at IS NOT NULL" 1 'versiones escritas por N+1 persistidas en la base'
printf '\nFASE 3: retroceso por binario, arranque normal N sin --migrate\n'
start "$TMP/bin/n"
probe retroceso
stop
[ "$VERSIONS_N1" = "$(versions_state)" ]; ok 'N no tocó versions/versions_at de los dispositivos existentes'
expect_sql "SELECT versions::text||'|'||coalesce(versions_at::text,'null') FROM devices WHERE name='lab-rollback'" "[]|null" 'dispositivo creado por N toma versions [] y versions_at NULL por defecto'
check_versions 1,2,3,4,5,6,7,8,9,10,11,12
ok 'historial 1–12 intacto tras arrancar N'
printf '\nFASE 4: negativo N --migrate sobre 0012\n'
dump_hash() {
  docker exec "$PG" pg_dump -U postgres -d atenea --no-owner --no-privileges |
    awk '!/^\\(un)?restrict /' | shasum -a 256 | cut -c1-64
}
BEFORE=$(dump_hash)
set +e
migrate "$TMP/bin/n" > "$TMP/negative.txt" 2>&1
NEGATIVE_RC=$?
set -e
printf 'NEGATIVO exit=%s; salida exacta:\n' "$NEGATIVE_RC"
awk '1' "$TMP/negative.txt"
[ "$NEGATIVE_RC" -ne 0 ]; ok 'N --migrate rechaza BD con 0012 (código distinto de 0)'
[ "$BEFORE" = "$(dump_hash)" ]; ok 'base completa e historial no cambiaron tras el negativo'
printf '\nFASE 5: re-upgrade N+1 idempotente y persistencia\n'
BEFORE=$(dump_hash)
migrate "$TMP/bin/n1"
[ "$BEFORE" = "$(dump_hash)" ]; ok 'migración N+1 idempotente, sin cambios en la base'
check_versions 1,2,3,4,5,6,7,8,9,10,11,12
ok 'historial final 1–12 exitoso'
[ "$HISTORY12" = "$(checksums 12)" ]; ok 'checksums 1–12 iguales tras la nueva subida'
start "$TMP/bin/n1"
probe reupgrade
stop
printf 'PASS retroceso 0012 completo en %s s; solo control plane/PostgreSQL sintéticos\n' "$((SECONDS-START))"
