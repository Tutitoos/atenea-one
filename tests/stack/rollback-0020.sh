#!/usr/bin/env bash
# Copyright 2026 Tutitoos
# SPDX-License-Identifier: Apache-2.0
# Retroceso v0.1.36 (migraciones 1–19) sobre la 0020 (disparador del orden de aprobación, ADR-031 adenda). No ejecuta down
# migrations: el binario anterior debe funcionar con el esquema que ya tiene el disparador. A diferencia del retroceso de la
# 0019, la 0.1.36 SÍ emite las conexiones automáticas y fija ella misma el orden de aprobación: aquí se mide que sigue
# haciéndolo con el disparador delante (mismo orden, mismas conexiones), que respeta las exclusiones y el ajuste, y que lo
# que aprueba durante el retroceso conserva su orden al volver a subir.
#
# N es el tag v0.1.36 y N+1 una revisión con la 0020 (`ATENEA_NEXT_REF`, por defecto HEAD). Como rollback-0019.sh, compila los
# dos control plane en el anfitrión y usa solo PostgreSQL 17 en Docker, con datos sintéticos y los binarios reales.
# Requiere docker, cargo, python3 y un openssl con Ed25519.
set -euo pipefail
ROOT=$(git -C "$(dirname "$0")" rev-parse --show-toplevel)
cd "$ROOT"
umask 077
START=$SECONDS
RUN="atenea-rollback-0020-$$"
PREVIOUS_TAG=${ATENEA_PREVIOUS_TAG:-v0.1.36}
NEXT_REF=${ATENEA_NEXT_REF:-HEAD}
TMP=$(mktemp -d "${TMPDIR:-/tmp}/atenea-rollback-0020.XXXXXX")
TMP=$(cd "$TMP" && pwd -P)
PG=$RUN-pg
PORT=${ATENEA_ROLLBACK_PG_PORT:-54180}
CP_PORT=${ATENEA_ROLLBACK_CP_PORT:-54181}
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
[ "$#" -eq 19 ]
set -- "$TMP/next"/apps/control-plane/migrations/*.sql
[ "$#" -ge 20 ]
test -f "$TMP/next/apps/control-plane/migrations/0020_approval_order_trigger.sql"
printf 'N %s commit=%s; N+1 %s commit=%s\n' "$PREVIOUS_TAG" "$PREVIOUS" "$NEXT_REF" "$NEXT"

# Un directorio de compilación por revisión (ver rollback-0019.sh).
mkdir -p "$TMP/bin"
build() {
  local out=$TARGET/rollback-0020/$(basename "$1")
  (cd "$1" && CARGO_TARGET_DIR=$out nice -n 19 cargo build --locked -j 2 -p atenea-control-plane "${@:3}")
  cp "$out/debug/atenea-control-plane" "$2"
}
build "$TMP/previous" "$TMP/bin/n"
build "$TMP/next" "$TMP/bin/n1" -p atenea-cli
cp "$TARGET/rollback-0020/next/debug/atenea" "$TMP/bin/atenea"
cmp -s "$TMP/bin/n" "$TMP/bin/n1" && { echo 'N y N+1 son el mismo binario' >&2; exit 1; }
printf 'N sha256=%s\nN+1 sha256=%s\n' "$(shasum -a 256 "$TMP/bin/n" | cut -c1-64)" "$(shasum -a 256 "$TMP/bin/n1" | cut -c1-64)"

# Secretos exclusivos de esta ejecución, nunca fixtures ni salida.
rnd() { od -An -N32 -tx1 /dev/urandom | tr -d ' \n'; }
PG_PASSWORD=$(rnd); APP_PASSWORD=$(rnd); SESSION_TOKEN=$(rnd); CSRF_TOKEN=$(rnd); ISSUER_SEED=$(rnd); WRAP_KEY=$(rnd)
docker run -d --name "$PG" -p "127.0.0.1:$PORT:5432" \
  -e "POSTGRES_PASSWORD=$PG_PASSWORD" -e POSTGRES_DB=atenea "$POSTGRES_IMAGE" >/dev/null
for ((i=0; i<60; i++)); do
  if docker exec "$PG" pg_isready -U postgres -d atenea >/dev/null 2>&1; then break; fi
  sleep 1
done
docker exec "$PG" pg_isready -U postgres -d atenea >/dev/null
printf '%s\n' "$ISSUER_SEED" > "$TMP/issuer.key"
printf '%s\n' "$WRAP_KEY" > "$TMP/wrap.key"

sql() { docker exec -i "$PG" psql -X -U postgres -d atenea -v ON_ERROR_STOP=1 -At "$@"; }
cp_vars=(ATENEA_CP_ISSUER_KEY_FILE="$TMP/issuer.key" ATENEA_CP_LISTEN="127.0.0.1:$CP_PORT"
  ATENEA_CP_PUBLIC_URL=https://control.example.invalid ATENEA_CP_APPLICATION_ROLE=atenea_app
  ATENEA_CP_ROOT_WRAP_KEY_FILE="$TMP/wrap.key" ATENEA_CP_REGISTRATION_OPEN=true
  RUST_LOG=atenea_control_plane=info)
migrate() { env "${cp_vars[@]}" ATENEA_CP_DATABASE_URL="postgres://postgres:$PG_PASSWORD@127.0.0.1:$PORT/atenea" "$1" --migrate; }
start() {
  env "${cp_vars[@]}" ATENEA_CP_DATABASE_URL="postgres://atenea_app:$APP_PASSWORD@127.0.0.1:$PORT/atenea" "$1" > "$TMP/cp.log" 2>&1 &
  CP_PID=$!
  sleep 1
  kill -0 "$CP_PID"
}
stop() { kill "$CP_PID"; wait "$CP_PID" 2>/dev/null || true; CP_PID=; }
versions_to() { local v=1 out=1; while [ "$v" -lt "$1" ]; do v=$((v + 1)); out="$out,$v"; done; printf '%s' "$out"; }
check_versions() {
  local actual
  actual=$(sql -c 'SELECT string_agg(version::text,'"'"','"'"' ORDER BY version) FROM _sqlx_migrations WHERE success')
  [ "$actual" = "$1" ] || { printf 'Migraciones inesperadas: %s\n' "$actual" >&2; return 1; }
}
ok() { printf 'PASS %s\n' "$1"; printf '.\n' >> "$TMP/checks"; }
expect_sql() { [ "$(sql -c "$1")" = "$2" ]; ok "$3"; }
checksums() { sql -c "SELECT string_agg(version::text||':'||encode(checksum,'hex'),',' ORDER BY version) FROM _sqlx_migrations WHERE version<=$1"; }

printf '\nFASE 1: N %s, migraciones 1–19 y siembra\n' "$PREVIOUS_TAG"
migrate "$TMP/bin/n"
check_versions "$(versions_to 19)"
ok 'N: historial 1–19 exitoso'
expect_sql "SELECT count(*) FROM pg_trigger WHERE tgname='devices_approval_order'" 0 'N: todavía no existe el disparador'
HISTORY=$(checksums 19)
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
        raise RuntimeError(f'{stage} {method} {path}: HTTP {e.code} {e.read()[:80]!r}') from None
for attempt in range(60):
    try:
        req('/healthz'); break
    except Exception:
        if attempt==59: raise
        time.sleep(1)
ck(True, 'healthz responde')
def enroll(name, net):
    cli('device-init','--dir',str(p/name))
    token=req(net+'/enrollment-tokens',{'ttl_secs':3600})['token']
    d=req('/api/v1/enroll',{'token':token,'bundle':(p/name/'bundle.bin').read_bytes().hex(),'name':'lab-'+name,'config_v3':True})
    s[name]=d['device_id']
    ck(d['status']=='pending',name+' inscrito pendiente por API')
    ck(req(net+'/devices/'+s[name]+'/approve',{})['status']=='approved',name+' aprobado')
def config(name):
    return req('/api/v1/device/config',device=name,raw=True)
def peers_in(b):
    """Pares de una configuración firmada (AOCF v1 + certificado 178 + longitud 4 + cuerpo + firma 64)."""
    body=b[187:-64]
    o=133
    for with_len in (False, True):
        n=body[o]; o+=1
        for _i in range(n):
            o+=1+(4 if body[o]==4 else 16)+(1 if with_len else 0)
    return int.from_bytes(body[o:o+2],'big')
def N(key):
    return '/api/v1/networks/'+s[key]
def autos(name):
    return sorted(req(N('auto')+'/devices/'+s[name]+'/auto-peers'))
if stage=='N':
    s['org']=req('/api/v1/orgs',{'name':'rollback-org'})['id']
    root=cli('root-init','--out',str(p/'root.key')).splitlines()[-1]
    s['auto']=req('/api/v1/orgs/'+s['org']+'/networks',{'name':'auto','ipv4_cidr':'10.46.0.0/24','ipv6_cidr':'fd46::/64','root_anchor':root})['id']
    ck(req(N('auto'))['auto_connect'] is True,'N: la red nace con conexiones automáticas')
    cli('issuer-cert','--root',str(p/'root.key'),'--issuer-key',os.environ['PROBE_ISSUER'],'--network-id',s['auto'].replace('-',''),'--serial','1','--valid-days','30','--out',str(p/'issuer.cert'))
    req(N('auto')+'/issuer-cert',(p/'issuer.cert').read_bytes(),'PUT',raw=True)
    for name in ('a','b','c'):
        enroll(name,N('auto'))
    ck(autos('a')==sorted([s['b'],s['c']]),'N: a se conecta automáticamente con b y c')
    ck(peers_in(config('a'))==2,'N: la configuración firmada de a lleva 2 pares sin ninguna política')
elif stage=='nuevo':
    ck(autos('a')==sorted([s['b'],s['c']]),'N+1: las conexiones automáticas de N siguen igual')
    ck(peers_in(config('a'))==2,'N+1: a sigue con 2 pares')
    req(N('auto')+'/devices/'+s['a']+'/auto-peers/'+s['c'],method='DELETE')
    ck(autos('a')==[s['b']],'N+1: la exclusión a–c corta solo esa pareja')
    req(N('auto'),{'auto_connect':False},'PATCH')
    enroll('d',N('auto'))
    ck(peers_in(config('d'))==0,'N+1: con el ajuste apagado d no se conecta con nadie')
    req(N('auto'),{'auto_connect':True},'PATCH')
elif stage=='retroceso':
    ck(autos('a')==[s['b']],'N sobre la 0020: sigue emitiendo las conexiones automáticas de a, sin la excluida')
    ck(peers_in(config('a'))==1 and peers_in(config('b'))==2,'N: la configuración firmada refleja la exclusión (a 1 par, b 2)')
    ck(peers_in(config('d'))==0,'N: d, aprobado con el ajuste apagado, sigue sin pares')
    ck(req('/api/v1/device/status?config_v3=true',{'paths':[]},device='b') is None,'N acepta el informe con ?config_v3=true')
    enroll('e',N('auto'))
    ck(peers_in(config('e'))==4,'N: e, aprobado durante el retroceso con el ajuste activo, se conecta con a, b, c y d')
    ck(autos('a')==sorted([s['b'],s['e']]),'N: a se conecta ahora también con e, y sigue sin c')
    req(N('auto')+'/devices/'+s['b']+'/auto-peers/'+s['e'],method='DELETE')
    ck(autos('b')==sorted([s['a'],s['c']]) and peers_in(config('e'))==3,'N: puede añadir exclusiones (b–e) sobre el esquema nuevo')
elif stage=='reupgrade':
    ck(autos('a')==sorted([s['b'],s['e']]),'N+1: a conserva a b y e, y la exclusión de c')
    ck(peers_in(config('e'))==3 and autos('e')==sorted([s['a'],s['c'],s['d']]),'N+1: e conserva lo aprobado por N y la exclusión b–e')
    ck(peers_in(config('d'))==1 and autos('d')==[s['e']],'N+1: d solo se conecta con e (aprobado después con el ajuste activo), no con a, b ni c')
    enroll('f',N('auto'))
    ck(peers_in(config('f'))==5,'N+1: una aprobación nueva se conecta con los cinco anteriores')
state_path.write_text(json.dumps(s))
PY
probe() {
  PROBE_DIR=$TMP PROBE_BASE="http://127.0.0.1:$CP_PORT" PROBE_CLI=$TMP/bin/atenea PROBE_ISSUER=$TMP/issuer.key \
    UPGRADE_SESSION=$SESSION_TOKEN UPGRADE_CSRF=$CSRF_TOKEN python3 "$TMP/probe.py" "$1"
}
start "$TMP/bin/n"
probe N
stop
cat > "$TMP/fingerprint.sql" <<'SQL'
SELECT md5(string_agg(v,E'\n' ORDER BY v)) FROM (
 SELECT to_jsonb(t)::text v FROM devices t
 UNION ALL SELECT to_jsonb(t)::text FROM networks t
 UNION ALL SELECT to_jsonb(t)::text FROM network_auto_exceptions t
 UNION ALL SELECT to_jsonb(t)::text FROM access_policies t
 UNION ALL SELECT to_jsonb(t)::text FROM ip_allocations t
) fixture;
SQL
BASELINE=$(sql < "$TMP/fingerprint.sql")
printf '\nFASE 2: upgrade N+1, 0020, exclusión, ajuste y un dispositivo con el ajuste apagado\n'
migrate "$TMP/bin/n1"
check_versions "$(versions_to 20)"
ok 'historial 1–20 exitoso'
[ "$HISTORY" = "$(checksums 19)" ]
ok 'checksums 1–19 intactos'
[ "$BASELINE" = "$(sql < "$TMP/fingerprint.sql")" ]; ok 'valores de toda la siembra intactos al migrar (la 0020 no reescribe datos)'
expect_sql "SELECT count(*) FROM pg_trigger WHERE tgname='devices_approval_order' AND NOT tgisinternal" 1 'el disparador existe tras la 0020'
start "$TMP/bin/n1"
probe nuevo
stop
expect_sql "SELECT string_agg(approval_order::text||':'||auto_connect_on_approval::text,',' ORDER BY approval_order) FROM devices" '1:true,2:true,3:true,4:false' 'orden y ajuste de a, b, c y d'
printf '\nFASE 3: retroceso por binario, arranque normal N sin --migrate\n'
start "$TMP/bin/n"
probe retroceso
stop
check_versions "$(versions_to 20)"
ok 'historial 1–20 intacto tras arrancar N'
expect_sql "SELECT approval_order||':'||auto_connect_on_approval FROM devices WHERE name='lab-e'" '5:true' 'e, aprobado por N, tiene el orden y el ajuste de N'
expect_sql "SELECT count(*) FROM network_auto_exceptions" 2 'las dos exclusiones están guardadas'
printf '\nFASE 4: negativo N --migrate sobre 0020\n'
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
[ "$NEGATIVE_RC" -ne 0 ]; ok 'N --migrate rechaza BD con 0020 (código distinto de 0)'
[ "$BEFORE" = "$(dump_hash)" ]; ok 'base completa e historial no cambiaron tras el negativo'
printf '\nFASE 5: re-upgrade N+1 idempotente y persistencia\n'
BEFORE=$(dump_hash)
migrate "$TMP/bin/n1"
[ "$BEFORE" = "$(dump_hash)" ]; ok 'migración N+1 idempotente, sin cambios en la base'
check_versions "$(versions_to 20)"
ok 'historial final 1–20 exitoso'
start "$TMP/bin/n1"
probe reupgrade
stop
expect_sql "SELECT string_agg(approval_order::text,',' ORDER BY approval_order) FROM devices" '1,2,3,4,5,6' 'los órdenes siguen siendo 1 a 6 sin huecos ni repetidos'
printf 'PASS retroceso 0020 completo en %s s; solo control plane/PostgreSQL sintéticos\n' "$((SECONDS-START))"
