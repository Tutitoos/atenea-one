#!/usr/bin/env bash
# Copyright 2026 Tutitoos
# SPDX-License-Identifier: Apache-2.0
# Retroceso v0.1.33 (migraciones 1–13) sobre 0014 (`device_grants.granted_by_device`, concesión delegada de permisos,
# ADR-027) con concesiones delegadas reales. No ejecuta down migrations: el binario anterior debe funcionar con el
# esquema ampliado. Deja medido qué NO hace el binario anterior: no retira lo que concedió quien revoca y, si un
# administrador reescribe una concesión delegada, no borra su autoría (`granted_by_device`).
#
# N es el tag v0.1.33 y N+1 el tag v0.1.34 (la primera versión con la 0014); `ATENEA_NEXT_REF` admite cualquier
# revisión (un commit de `main`, por ejemplo). Como rollback-0010.sh, rollback-0012.sh y rollback-0013.sh, el ensayo
# describe un paso concreto del historial. Compila los dos control plane en el anfitrión (perfil dev, un solo
# CARGO_TARGET_DIR) y usa solo PostgreSQL 17 en Docker, con datos sintéticos y los binarios reales del control plane y
# de la CLI (la delegación se ejercita con peticiones firmadas AtDevice de verdad). Requiere docker, cargo, python3 y un
# openssl con Ed25519 (`pkeyutl -rawin`).
set -euo pipefail
ROOT=$(git -C "$(dirname "$0")" rev-parse --show-toplevel)
cd "$ROOT"
umask 077
START=$SECONDS
RUN="atenea-rollback-0014-$$"
PREVIOUS_TAG=${ATENEA_PREVIOUS_TAG:-v0.1.33}
NEXT_REF=${ATENEA_NEXT_REF:-v0.1.34}
TMP=$(mktemp -d "${TMPDIR:-/tmp}/atenea-rollback-0014.XXXXXX")
TMP=$(cd "$TMP" && pwd -P)
PG=$RUN-pg
PORT=${ATENEA_ROLLBACK_PG_PORT:-54176}
CP_PORT=${ATENEA_ROLLBACK_CP_PORT:-54177}
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
[ "$#" -eq 13 ]
test -f "$TMP/previous/apps/control-plane/migrations/0013_device_grants.sql"
set -- "$TMP/next"/apps/control-plane/migrations/*.sql
[ "$#" -eq 14 ]
test -f "$TMP/next/apps/control-plane/migrations/0014_device_grants_delegated.sql"
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

printf '\nFASE 1: N %s, migraciones 1–13 y siembra\n' "$PREVIOUS_TAG"
migrate "$TMP/bin/n"
check_versions 1,2,3,4,5,6,7,8,9,10,11,12,13
ok 'N: historial 1–13 exitoso'
HISTORY=$(checksums 13)
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
SIX=['read:devices','read:policies','write:join-codes','write:approve-devices','write:policies','write:revoke-devices']
def put_grant(name, scopes, scope, device=None):
    path = (ADMIN+'/devices/'+s[name]+'/grant') if device else (net+'/devices/'+s[name]+'/grant')
    return req(path, {'scopes':scopes,'scope':scope}, 'PUT', device=device)
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
    for name in ('lead','peer','third'):
        enroll(name,approve=True)
    enroll('pending')
    s['policy']=req(net+'/policies',policy)['id']
    put_grant('lead', SIX, {'kind':'network'})
    ck(len(req(ADMIN+'/grant',device='lead')['scopes'])==6,'siembra: el delegado lee su concesión de la 0.1.33')
    ck(fails('invalid_grant',net+'/devices/'+s['third']+'/grant',{'scopes':['read:devices','write:grants'],'scope':{'kind':'network'}},'PUT'),'N no conoce write:grants (400 invalid_grant)')
    ck(fails('HTTP 404',ADMIN+'/grants',device='lead'),'N no tiene la ruta de concesiones de un delegado (404)')
elif stage=='nuevo':
    ck(all(g['granted_by_device'] is None for g in req(net+'/grants')),'N+1: las concesiones migradas no tienen autor delegado')
    g=put_grant('lead',['read:devices','read:policies','write:grants'],{'kind':'devices','devices':[s['peer'],s['third']]})
    ck(g['granted_by_device'] is None and 'write:grants' in g['scopes'],'N+1: el administrador concede write:grants')
    put_grant('third',['read:devices'],{'kind':'enrolled'})
    r=put_grant('peer',['read:devices'],{'kind':'devices','devices':[s['third']]},device='lead')
    ck(r['granted_by_device']==s['lead'] and 'granted_by' not in r,'N+1: el delegado concede dentro de su alcance, con su autoría y sin el usuario')
    ids={x['device_id'] for x in req(ADMIN+'/grants',device='lead')}
    ck(ids=={s['peer'],s['third']},'N+1: el delegado solo ve las concesiones de su alcance')
    ck(fails('grant_exceeds_own',ADMIN+'/devices/'+s['peer']+'/grant',{'scopes':['write:policies'],'scope':{'kind':'devices','devices':[s['third']]}},'PUT','lead'),'N+1: no da permisos que no tiene')
    ck(fails('grant_exceeds_own',ADMIN+'/devices/'+s['peer']+'/grant',{'scopes':['write:grants'],'scope':{'kind':'devices','devices':[s['third']]}},'PUT','lead'),'N+1: no da write:grants')
    ck(fails('grant_exceeds_own',ADMIN+'/devices/'+s['peer']+'/grant',{'scopes':['read:devices'],'scope':{'kind':'network'}},'PUT','lead'),'N+1: no da un alcance mayor')
    ck(fails('grant_self',ADMIN+'/devices/'+s['lead']+'/grant',{'scopes':['read:devices'],'scope':{'kind':'devices','devices':[s['third']]}},'PUT','lead'),'N+1: no cambia su propia concesión')
    ck(fails('grant_not_yours',ADMIN+'/devices/'+s['third']+'/grant',{'scopes':['read:devices'],'scope':{'kind':'devices','devices':[s['third']]}},'PUT','lead'),'N+1: no toca la concesión de un administrador')
    ck(fails('out_of_scope',ADMIN+'/devices/'+s['pending']+'/grant',{'scopes':['read:devices'],'scope':{'kind':'devices','devices':[s['third']]}},'PUT','lead'),'N+1: no concede fuera de su alcance')
    ck(len(req(ADMIN+'/devices',device='peer'))==4,'N+1: lo concedido por el delegado se ejerce')
    actors={(e['action'],e['actor_kind']) for e in req(net+'/audit')}
    ck({('device.grant','user'),('device.grant','device')}<=actors,'N+1: la auditoría distingue al administrador y al dispositivo que concede')
elif stage=='retroceso':
    ck(any(o['id']==s['org'] for o in req('/api/v1/orgs')),'organización legible')
    ck(req(net)['name']=='rollback-net','detalle de red legible')
    ck(len(req(net+'/devices'))==4,'dispositivos legibles (cuatro)')
    rows=req(net+'/grants')
    ck(any(x['device_id']==s['peer'] for x in rows) and any(x['device_id']==s['lead'] for x in rows),'N lista también las concesiones delegadas (lee granted_by como usuario)')
    ck(len(req(ADMIN+'/devices',device='peer'))==4,'N ejerce una concesión que dio un delegado')
    ck(len(req('/api/v1/device/config',device='peer',raw=True))>100,'N emite configuración firmada')
    ck(fails('HTTP 404',ADMIN+'/grants',device='lead'),'N no tiene las rutas de concesión del delegado (404)')
    ck(fails('HTTP 404',ADMIN+'/devices/'+s['peer']+'/grant',{'scopes':['read:devices'],'scope':{'kind':'network'}},'PUT','lead'),'N: el delegado no puede conceder (404)')
    ck(fails('invalid_grant',net+'/devices/'+s['third']+'/grant',{'scopes':['write:grants'],'scope':{'kind':'network'}},'PUT'),'N rechaza write:grants al administrador (400 invalid_grant)')
    enroll('rollback',approve=True)
    ck(len(req('/api/v1/device/config',device='rollback',raw=True))>100,'N emite configuración a un dispositivo nuevo')
    # N reescribe la concesión que dio el delegado, como administrador: no toca la autoría (limitación conocida).
    g=put_grant('peer',['read:devices','read:policies'],{'kind':'enrolled'})
    ck(g['scope']['kind']=='enrolled' and len(g['scopes'])==2,'N: el administrador sustituye la concesión del delegado')
    # N revoca al delegado: borra su concesión y NO la que él dio.
    ck(req(net+'/devices/'+s['lead']+'/revoke',{})['status']=='revoked','N revoca al delegado')
elif stage=='reupgrade':
    ck(len(req(net+'/devices'))==5,'persistencia: cinco dispositivos')
    ck(fails('device_not_approved',ADMIN+'/grant',device='lead'),'N+1: el delegado revocado por N no ejerce nada (403 device_not_approved)')
    ck(len(req(ADMIN+'/devices',device='peer'))==5,'N+1: la concesión que dejó el delegado revocado sigue ejerciéndose (limitación conocida)')
    row=[x for x in req(net+'/grants') if x['device_id']==s['peer']][0]
    ck(row['granted_by_device']==s['lead'],'N+1: el administrador ve esa concesión atribuida al delegado revocado (limitación conocida)')
    ck(req(net+'/devices/'+s['peer']+'/grant',method='DELETE') is None,'N+1: un administrador puede retirar la concesión huérfana')
    ck(not any(x['device_id']==s['peer'] for x in req(net+'/grants')),'N+1: la concesión huérfana desaparece')
    put_grant('third',['read:devices','read:policies','write:grants'],{'kind':'network'})
    r=put_grant('peer',['read:devices'],{'kind':'enrolled'},device='third')
    ck(r['granted_by_device']==s['third'],'N+1: tras el retroceso un delegado nuevo vuelve a conceder')
    g=put_grant('peer',['read:devices','read:policies'],{'kind':'network'})
    ck(g['granted_by_device'] is None,'N+1: una concesión reescrita por un administrador pierde la autoría delegada')
    put_grant('third',['read:devices'],{'kind':'enrolled'})
    put_grant('third',['read:devices','read:policies','write:grants'],{'kind':'network'})
    put_grant('rollback',['read:devices'],{'kind':'enrolled'},device='third')
    put_grant('third',['read:devices'],{'kind':'enrolled'})
    ck(not any(x['device_id']==s['rollback'] for x in req(net+'/grants')),'N+1: cambiar la concesión de quien concede retira lo que dio (cascada)')
state_path.write_text(json.dumps(s))
PY
probe() {
  PROBE_DIR=$TMP PROBE_BASE="http://127.0.0.1:$CP_PORT" PROBE_CLI=$TMP/bin/atenea PROBE_ISSUER=$TMP/issuer.key \
    UPGRADE_SESSION=$SESSION_TOKEN UPGRADE_CSRF=$CSRF_TOKEN python3 "$TMP/probe.py" "$1"
}
start "$TMP/bin/n"
probe N
stop
# Toda la siembra salvo la autoría delegada de las concesiones; en N no existe y la resta es una no-operación.
cat > "$TMP/fingerprint.sql" <<'SQL'
SELECT md5(string_agg(v,E'\n' ORDER BY v)) FROM (
 SELECT to_jsonb(t)::text v FROM devices t
 UNION ALL SELECT to_jsonb(t)::text FROM enrollment_tokens t
 UNION ALL SELECT to_jsonb(t)::text FROM users t
 UNION ALL SELECT to_jsonb(t)::text FROM organizations t
 UNION ALL SELECT to_jsonb(t)::text FROM networks t
 UNION ALL SELECT to_jsonb(t)::text FROM access_policies t
 UNION ALL SELECT to_jsonb(t)::text FROM memberships t
 UNION ALL SELECT (to_jsonb(t)-'granted_by_device')::text FROM device_grants t
) fixture;
SQL
BASELINE=$(sql < "$TMP/fingerprint.sql")
printf '\nFASE 2: upgrade N+1, 0014 y concesión delegada real\n'
migrate "$TMP/bin/n1"
check_versions 1,2,3,4,5,6,7,8,9,10,11,12,13,14
ok 'historial 1–14 exitoso'
[ "$HISTORY" = "$(checksums 13)" ]
ok 'checksums 1–13 intactos'
HISTORY14=$(checksums 14)
[ "$BASELINE" = "$(sql < "$TMP/fingerprint.sql")" ]; ok 'valores de toda la siembra intactos al migrar'
expect_sql "SELECT count(*) FROM device_grants WHERE granted_by_device IS NOT NULL" 0 'ninguna concesión migrada tiene autor delegado'
expect_sql "SELECT is_nullable FROM information_schema.columns WHERE table_name='device_grants' AND column_name='granted_by_device'" YES 'granted_by_device es nulable'
expect_sql "SELECT is_nullable FROM information_schema.columns WHERE table_name='device_grants' AND column_name='granted_by'" NO 'granted_by sigue siendo no nulo (el binario anterior lo lee como uuid)'
start "$TMP/bin/n1"
probe nuevo
stop
GRANTS_N1=$(sql -c "SELECT string_agg(device_id::text||':'||scopes::text||':'||scope_kind||':'||coalesce(granted_by_device::text,'-'),',' ORDER BY device_id) FROM device_grants")
expect_sql "SELECT count(*) FROM device_grants" 3 'tres concesiones persisten en la base (delegado, administrador y delegada)'
expect_sql "SELECT count(*) FROM device_grants WHERE granted_by_device IS NOT NULL" 1 'una concesión lleva autor delegado'
printf '\nFASE 3: retroceso por binario, arranque normal N sin --migrate\n'
start "$TMP/bin/n"
probe retroceso
stop
check_versions 1,2,3,4,5,6,7,8,9,10,11,12,13,14
ok 'historial 1–14 intacto tras arrancar N'
expect_sql "SELECT count(*) FROM device_grants WHERE device_id=(SELECT id FROM devices WHERE name='lab-lead')" 0 'N borró la concesión del delegado que revocó'
expect_sql "SELECT count(*) FROM device_grants g JOIN devices d ON d.id=g.granted_by_device WHERE d.status='revoked'" 1 'N NO retiró lo que concedió el delegado revocado: queda una concesión huérfana'
expect_sql "SELECT string_agg(scopes::text,',') FROM device_grants WHERE device_id=(SELECT id FROM devices WHERE name='lab-peer')" '["read:devices", "read:policies"]' 'N reescribió la concesión de la que quedó huérfana'
[ "$GRANTS_N1" != "$(sql -c "SELECT string_agg(device_id::text||':'||scopes::text||':'||scope_kind||':'||coalesce(granted_by_device::text,'-'),',' ORDER BY device_id) FROM device_grants")" ]; ok 'las concesiones cambiaron solo por lo que hizo N'
printf '\nFASE 4: negativo N --migrate sobre 0014\n'
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
[ "$NEGATIVE_RC" -ne 0 ]; ok 'N --migrate rechaza BD con 0014 (código distinto de 0)'
[ "$BEFORE" = "$(dump_hash)" ]; ok 'base completa e historial no cambiaron tras el negativo'
printf '\nFASE 5: re-upgrade N+1 idempotente y persistencia\n'
BEFORE=$(dump_hash)
migrate "$TMP/bin/n1"
[ "$BEFORE" = "$(dump_hash)" ]; ok 'migración N+1 idempotente, sin cambios en la base'
check_versions 1,2,3,4,5,6,7,8,9,10,11,12,13,14
ok 'historial final 1–14 exitoso'
[ "$HISTORY14" = "$(checksums 14)" ]; ok 'checksums 1–14 iguales tras la nueva subida'
start "$TMP/bin/n1"
probe reupgrade
stop
printf 'PASS retroceso 0014 completo en %s s; solo control plane/PostgreSQL sintéticos\n' "$((SECONDS-START))"
