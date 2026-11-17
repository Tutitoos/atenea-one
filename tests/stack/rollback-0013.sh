#!/usr/bin/env bash
# Copyright 2026 Tutitoos
# SPDX-License-Identifier: Apache-2.0
# Retroceso v0.1.32 (migraciones 1–12) sobre 0013 (device_grants y la autoría delegada, ADR-026) con escrituras
# nuevas y administración delegada real. No ejecuta down migrations: el binario anterior debe funcionar con el esquema
# ampliado.
#
# N es el tag v0.1.32 y N+1 el tag v0.1.33 (la primera versión con la 0013); `ATENEA_NEXT_REF` admite cualquier revisión
# (un commit de `main`, por ejemplo). Como rollback-0010.sh y rollback-0012.sh, el ensayo describe un paso concreto del
# historial. Compila los dos control plane en el anfitrión (perfil dev, un solo
# CARGO_TARGET_DIR) y usa solo PostgreSQL 17 en Docker, con datos sintéticos y los binarios reales del control plane y
# de la CLI (la delegación se ejercita con peticiones firmadas AtDevice de verdad). Requiere docker, cargo, python3 y un
# openssl con Ed25519 (`pkeyutl -rawin`).
set -euo pipefail
ROOT=$(git -C "$(dirname "$0")" rev-parse --show-toplevel)
cd "$ROOT"
umask 077
START=$SECONDS
RUN="atenea-rollback-0013-$$"
PREVIOUS_TAG=${ATENEA_PREVIOUS_TAG:-v0.1.32}
NEXT_REF=${ATENEA_NEXT_REF:-v0.1.33}
TMP=$(mktemp -d "${TMPDIR:-/tmp}/atenea-rollback-0013.XXXXXX")
TMP=$(cd "$TMP" && pwd -P)
PG=$RUN-pg
PORT=${ATENEA_ROLLBACK_PG_PORT:-54174}
CP_PORT=${ATENEA_ROLLBACK_CP_PORT:-54175}
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
  for tree in previous next; do
    if [ -d "$TMP/$tree" ]; then git worktree remove --force "$TMP/$tree"; fi
  done
  rm -rf "$TMP"
  exit "$rc"
}
trap cleanup EXIT
trap 'exit 130' INT
trap 'exit 143' TERM

docker info >/dev/null
PREVIOUS=$(git rev-parse "$PREVIOUS_TAG^{commit}")
NEXT=$(git rev-parse "$NEXT_REF^{commit}")
git worktree add --detach "$TMP/previous" "$PREVIOUS"
git worktree add --detach "$TMP/next" "$NEXT"
set -- "$TMP/previous"/apps/control-plane/migrations/*.sql
[ "$#" -eq 12 ]
test -f "$TMP/previous/apps/control-plane/migrations/0012_device_versions.sql"
set -- "$TMP/next"/apps/control-plane/migrations/*.sql
[ "$#" -eq 13 ]
test -f "$TMP/next/apps/control-plane/migrations/0013_device_grants.sql"
printf 'N %s commit=%s; N+1 %s commit=%s\n' "$PREVIOUS_TAG" "$PREVIOUS" "$NEXT_REF" "$NEXT"

# Binarios: cada revisión se compila con su propio árbol; la CLI es la de N+1 (device-init, root-init, issuer-cert).
mkdir -p "$TMP/bin"
build() {
  (cd "$1" && CARGO_TARGET_DIR=$TARGET nice -n 19 cargo build --locked -j 2 -p atenea-control-plane "${@:3}")
  cp "$TARGET/debug/atenea-control-plane" "$2"
}
build "$TMP/previous" "$TMP/bin/n"
build "$TMP/next" "$TMP/bin/n1" -p atenea-cli
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

printf '\nFASE 1: N %s, migraciones 1–12 y siembra\n' "$PREVIOUS_TAG"
migrate "$TMP/bin/n"
check_versions 1,2,3,4,5,6,7,8,9,10,11,12
ok 'N: historial 1–12 exitoso'
HISTORY=$(checksums 12)
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
ADMIN = '/api/v1/device/admin'
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
        raise RuntimeError(f'{stage} {method} {path}: HTTP {e.code} {e.read()[:80]!r}') from None
def fails(code, path, body=None, method=None, device=None):
    """La petición debe fallar con ese texto (código HTTP o código de la API)."""
    try:
        req(path, body, method, device)
    except RuntimeError as e:
        return code in str(e)
    return False
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
def init(name):
    cli('device-init','--dir',str(p/name))
def enroll_with(name, token):
    init(name)
    d=req('/api/v1/enroll',{'token':token,'bundle':(p/name/'bundle.bin').read_bytes().hex(),'name':'lab-'+name})
    s[name]=d['device_id']
    ck(d['status']=='pending',name+' inscrito pendiente por API')
def enroll(name, approve=False):
    enroll_with(name, req(net+'/enrollment-tokens',{'ttl_secs':3600})['token'])
    if approve:
        ck(req(net+'/devices/'+s[name]+'/approve',{})['status']=='approved',name+' aprobado')
policy={'src':{'kind':'any'},'dst':{'kind':'any'},'proto':0,'port_min':0,'port_max':0}
def pair(a, b, port=22):
    return {'src':{'kind':'device','device_id':s[a]},'dst':{'kind':'device','device_id':s[b]},'proto':6,'port_min':port,'port_max':port}
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
    enroll('lead',approve=True)
    enroll('peer',approve=True)
    enroll('pending')
    s['policy']=req(net+'/policies',policy)['id']
    ck(any(d['id']==s['pending'] and d['status']=='pending' for d in req(net+'/devices')),'siembra: dos aprobados, uno pendiente y una política de administrador')
    ck(fails('HTTP 404',ADMIN+'/grant',device='lead'),'N no tiene las rutas de administración delegada (404)')
elif stage=='nuevo':
    devices=req(net+'/devices')
    ck(len(devices)==3 and all(d['enrolled_by_device'] is None for d in devices),'N+1: dispositivos migrados con enrolled_by_device nulo')
    ck(all(x['created_by_device'] is None for x in req(net+'/policies')),'N+1: políticas migradas con created_by_device nulo')
    ck(req(net+'/grants')==[],'N+1: sin concesiones tras migrar')
    ck(fails('grant_missing',ADMIN+'/grant',device='lead'),'N+1: sin concesión, 403 grant_missing')
    g=req(net+'/devices/'+s['lead']+'/grant',{'scopes':['read:devices','read:policies','write:join-codes','write:approve-devices','write:policies','write:revoke-devices'],'scope':{'kind':'enrolled'}},'PUT')
    ck(g['scope']['kind']=='enrolled' and len(g['scopes'])==6,'N+1: el administrador concede permisos con alcance enrolled')
    ck(req(ADMIN+'/grant',device='lead')['scope']['kind']=='enrolled','N+1: el delegado lee su concesión con su firma')
    inv=req(ADMIN+'/devices',device='lead')
    ck(len(inv)==3 and [d['id'] for d in inv if d['in_scope']]==[s['lead']],'N+1: el inventario cubre la red y solo él está en su alcance')
    ck(fails('grant_missing',ADMIN+'/devices',device='peer'),'N+1: otro dispositivo sin concesión sigue sin permisos')
    code=req(ADMIN+'/join-codes',{'ttl_secs':600},device='lead')
    ck(bool(code['token']) and bool(code['code']),'N+1: el delegado genera un código de unión')
    enroll_with('delegated',code['token'])
    ck(any(d['id']==s['delegated'] and d['enrolled_by_device']==s['lead'] for d in req(net+'/devices')),'N+1: el administrador ve quién dio de alta al nuevo')
    fp={d['id']:d['fingerprint'] for d in req(ADMIN+'/devices',device='lead')}
    ck(fails('fingerprint_mismatch',ADMIN+'/devices/'+s['delegated']+'/approve',{'fingerprint':fp[s['peer']]},device='lead'),'N+1: una huella distinta no aprueba')
    ck(fails('out_of_scope',ADMIN+'/devices/'+s['pending']+'/approve',{'fingerprint':fp[s['pending']]},device='lead'),'N+1: un pendiente de otro código queda fuera de alcance')
    ck(req(ADMIN+'/devices/'+s['delegated']+'/approve',{'fingerprint':fp[s['delegated']]},device='lead')['status']=='approved','N+1: el delegado aprueba por huella lo que dio de alta')
    s['delegated_policy']=req(ADMIN+'/policies',pair('lead','delegated'),device='lead')['id']
    ck(fails('out_of_scope',ADMIN+'/policies',pair('lead','peer'),device='lead'),'N+1: no puede nombrar a un dispositivo ajeno')
    ck(fails('out_of_scope',ADMIN+'/policies',policy,device='lead'),'N+1: no crea políticas any')
    ck(fails('out_of_scope',ADMIN+'/policies/'+s['policy'],method='DELETE',device='lead'),'N+1: no borra la política de un administrador')
    ck(any(x['id']==s['delegated_policy'] and x['created_by_device']==s['lead'] for x in req(net+'/policies')),'N+1: la política delegada lleva a su autor')
    ck(any(d['device_id']==s['delegated'] for d in req('/api/v1/device/peers',device='lead')),'N+1: el equipo nuevo aparece como par del delegado')
    actors={(e['action'],e['actor_kind']) for e in req(net+'/audit')}
    ck({('token.create','device'),('device.approve','device'),('policy.create','device'),('device.grant','user')}<=actors,'N+1: la auditoría registra al dispositivo como autor')
elif stage=='retroceso':
    ck(any(o['id']==s['org'] for o in req('/api/v1/orgs')),'organización legible')
    ck(req(net)['name']=='rollback-net','detalle de red legible')
    devices=req(net+'/devices')
    ck(len(devices)==4,'dispositivos legibles (cuatro)')
    ck(any(x['id']==s['delegated_policy'] for x in req(net+'/policies')),'la política creada por un delegado se lee')
    ck(isinstance(req(net+'/audit'),list),'auditoría legible con eventos de dispositivo')
    ck(any(d['device_id']==s['peer'] for d in req('/api/v1/device/peers',device='lead')),'N: /device/peers firmado sigue sirviendo')
    ck(len(req('/api/v1/device/config',device='peer',raw=True))>100,'N emite configuración firmada')
    ck(fails('HTTP 404',ADMIN+'/grant',device='lead'),'N no ejerce la concesión (rutas inexistentes, 404)')
    enroll('rollback',approve=True)
    ck(len(req('/api/v1/device/config',device='rollback',raw=True))>100,'N emite configuración a un dispositivo nuevo')
    temporary=req(net+'/policies',dict(policy,proto=6,port_min=443,port_max=443))
    req(net+'/policies/'+temporary['id'],method='DELETE')
    ck(not any(x['id']==temporary['id'] for x in req(net+'/policies')),'N crea y borra una política de administrador')
    req(net+'/policies/'+s['delegated_policy'],method='DELETE')
    ck(not any(x['id']==s['delegated_policy'] for x in req(net+'/policies')),'N borra una política creada por un delegado')
    # N no conoce las concesiones: revocar al delegado deja su fila huérfana (comprobado por SQL fuera del cliente).
    ck(req(net+'/devices/'+s['lead']+'/revoke',{})['status']=='revoked','N revoca al delegado')
elif stage=='reupgrade':
    devices=req(net+'/devices')
    ck(len(devices)==5,'persistencia: cinco dispositivos')
    ck(any(d['id']==s['delegated'] and d['enrolled_by_device']==s['lead'] for d in devices),'N+1 vuelve a leer quién dio de alta al nuevo')
    ck(any(d['id']==s['lead'] and d['status']=='revoked' for d in devices),'el delegado revocado por N sigue revocado')
    ck(fails('device_not_approved',ADMIN+'/grant',device='lead'),'N+1: la concesión huérfana de un revocado no se ejerce (403 device_not_approved)')
    ck(any(x['device_id']==s['lead'] for x in req(net+'/grants')),'la fila huérfana sigue listada para el administrador (limitación conocida)')
    ck(fails('device_not_approved',net+'/devices/'+s['lead']+'/grant',{'scopes':['read:devices'],'scope':{'kind':'enrolled'}},'PUT'),'N+1: no se concede a un revocado (409)')
    req(net+'/devices/'+s['peer']+'/grant',{'scopes':['read:devices'],'scope':{'kind':'network'}},'PUT')
    ck(len(req(ADMIN+'/devices',device='peer'))==5,'N+1: una concesión nueva funciona tras el retroceso')
    ck(fails('grant_missing',ADMIN+'/policies',pair('peer','delegated'),device='peer'),'N+1: sin write:policies no crea políticas (403 grant_missing)')
state_path.write_text(json.dumps(s))
PY
probe() {
  PROBE_DIR=$TMP PROBE_BASE="http://127.0.0.1:$CP_PORT" PROBE_CLI=$TMP/bin/atenea PROBE_ISSUER=$TMP/issuer.key \
    UPGRADE_SESSION=$SESSION_TOKEN UPGRADE_CSRF=$CSRF_TOKEN python3 "$TMP/probe.py" "$1"
}
start "$TMP/bin/n"
probe N
stop
# Toda la siembra salvo las columnas de autoría; en N no existen y la resta es una no-operación.
cat > "$TMP/fingerprint.sql" <<'SQL'
SELECT md5(string_agg(v,E'\n' ORDER BY v)) FROM (
 SELECT (to_jsonb(t)-'enrolled_by_device')::text v FROM devices t
 UNION ALL SELECT (to_jsonb(t)-'created_by_device')::text FROM enrollment_tokens t
 UNION ALL SELECT to_jsonb(t)::text FROM users t
 UNION ALL SELECT to_jsonb(t)::text FROM organizations t
 UNION ALL SELECT to_jsonb(t)::text FROM networks t
 UNION ALL SELECT (to_jsonb(t)-'created_by_device')::text FROM access_policies t
 UNION ALL SELECT to_jsonb(t)::text FROM memberships t
) fixture;
SQL
BASELINE=$(sql < "$TMP/fingerprint.sql")
printf '\nFASE 2: upgrade N+1, 0013 y administración delegada real\n'
migrate "$TMP/bin/n1"
check_versions 1,2,3,4,5,6,7,8,9,10,11,12,13
ok 'historial 1–13 exitoso'
[ "$HISTORY" = "$(checksums 12)" ]
ok 'checksums 1–12 intactos'
HISTORY13=$(checksums 13)
[ "$BASELINE" = "$(sql < "$TMP/fingerprint.sql")" ]; ok 'valores de toda la siembra intactos al migrar'
expect_sql "SELECT count(*) FROM device_grants" 0 'device_grants existe y está vacía'
expect_sql "SELECT string_agg(table_name||'.'||column_name||':'||is_nullable,',' ORDER BY table_name,column_name) FROM information_schema.columns WHERE (table_name,column_name) IN (('access_policies','created_by'),('access_policies','created_by_device'),('enrollment_tokens','created_by'),('enrollment_tokens','created_by_device'),('devices','enrolled_by_device'))" 'access_policies.created_by:YES,access_policies.created_by_device:YES,devices.enrolled_by_device:YES,enrollment_tokens.created_by:YES,enrollment_tokens.created_by_device:YES' 'columnas de autoría nulables y created_by admite nulo'
start "$TMP/bin/n1"
probe nuevo
stop
GRANTS_N1=$(sql -c "SELECT string_agg(device_id::text||':'||scopes::text||':'||scope_kind,',' ORDER BY device_id) FROM device_grants")
expect_sql "SELECT count(*) FROM device_grants" 1 'la concesión del administrador persiste en la base'
printf '\nFASE 3: retroceso por binario, arranque normal N sin --migrate\n'
start "$TMP/bin/n"
probe retroceso
stop
check_versions 1,2,3,4,5,6,7,8,9,10,11,12,13
ok 'historial 1–13 intacto tras arrancar N'
expect_sql "SELECT count(*) FROM device_grants g JOIN devices d ON d.id=g.device_id WHERE d.status='revoked'" 1 'N dejó huérfana la concesión del delegado que revocó (N no conoce device_grants)'
expect_sql "SELECT count(*) FROM access_policies WHERE created_by IS NULL AND created_by_device IS NULL" 0 'toda política conserva un autor (usuario o dispositivo) tras escribir N'
[ "$GRANTS_N1" = "$(sql -c "SELECT string_agg(device_id::text||':'||scopes::text||':'||scope_kind,',' ORDER BY device_id) FROM device_grants")" ]; ok 'N no modificó las concesiones'
printf '\nFASE 4: negativo N --migrate sobre 0013\n'
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
[ "$NEGATIVE_RC" -ne 0 ]; ok 'N --migrate rechaza BD con 0013 (código distinto de 0)'
[ "$BEFORE" = "$(dump_hash)" ]; ok 'base completa e historial no cambiaron tras el negativo'
printf '\nFASE 5: re-upgrade N+1 idempotente y persistencia\n'
BEFORE=$(dump_hash)
migrate "$TMP/bin/n1"
[ "$BEFORE" = "$(dump_hash)" ]; ok 'migración N+1 idempotente, sin cambios en la base'
check_versions 1,2,3,4,5,6,7,8,9,10,11,12,13
ok 'historial final 1–13 exitoso'
[ "$HISTORY13" = "$(checksums 13)" ]; ok 'checksums 1–13 iguales tras la nueva subida'
start "$TMP/bin/n1"
probe reupgrade
stop
printf 'PASS retroceso 0013 completo en %s s; solo control plane/PostgreSQL sintéticos\n' "$((SECONDS-START))"
