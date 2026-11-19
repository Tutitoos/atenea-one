#!/usr/bin/env bash
# Copyright 2026 Tutitoos
# SPDX-License-Identifier: Apache-2.0
# Retroceso v0.1.34 (migraciones 1–14) sobre 0015 a 0018 (cuentas propias y miembros por red de ADR-028, redes
# gestionadas de ADR-029 y direccionamiento elegible). No ejecuta down migrations: el binario anterior debe funcionar con
# el esquema ampliado. Deja medido qué NO sabe hacer el binario anterior con lo que solo existe desde la 0.1.35: una
# red solo IPv4 (`ipv6_cidr` nulo), una red gestionada (no renueva su certificado) y las cuentas propias.
#
# N es el tag v0.1.34 y N+1 el tag v0.1.35 (la primera versión con la 0015 a la 0018); `ATENEA_NEXT_REF` admite
# cualquier revisión. Como rollback-0014.sh, compila los dos control plane en el anfitrión y usa solo PostgreSQL 17 en
# Docker, con datos sintéticos y los binarios reales. Requiere docker, cargo, python3 y un openssl con Ed25519.
set -euo pipefail
ROOT=$(git -C "$(dirname "$0")" rev-parse --show-toplevel)
cd "$ROOT"
umask 077
START=$SECONDS
RUN="atenea-rollback-0018-$$"
PREVIOUS_TAG=${ATENEA_PREVIOUS_TAG:-v0.1.34}
NEXT_REF=${ATENEA_NEXT_REF:-v0.1.35}
TMP=$(mktemp -d "${TMPDIR:-/tmp}/atenea-rollback-0018.XXXXXX")
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
[ "$#" -eq 14 ]
test -f "$TMP/previous/apps/control-plane/migrations/0014_device_grants_delegated.sql"
set -- "$TMP/next"/apps/control-plane/migrations/*.sql
[ "$#" -eq 18 ]
test -f "$TMP/next/apps/control-plane/migrations/0018_network_addressing.sql"
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
PG_PASSWORD=$(rnd); APP_PASSWORD=$(rnd); SESSION_TOKEN=$(rnd); CSRF_TOKEN=$(rnd); ISSUER_SEED=$(rnd); WRAP_KEY=$(rnd); LOCAL_PASSWORD=$(rnd)
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

printf '\nFASE 1: N %s, migraciones 1–14 y siembra\n' "$PREVIOUS_TAG"
migrate "$TMP/bin/n"
check_versions 1,2,3,4,5,6,7,8,9,10,11,12,13,14
ok 'N: historial 1–14 exitoso'
HISTORY=$(checksums 14)
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
def req(path, body=None, method=None, device=None, raw=False, anon=False):
    method = method or ('POST' if body is not None else 'GET')
    data = body if isinstance(body, bytes) else (json.dumps(body).encode() if body is not None else b'')
    headers = {'Cookie':'atenea_session='+os.environ['UPGRADE_SESSION'], 'X-CSRF-Token':os.environ['UPGRADE_CSRF'], 'Content-Type':'application/json'}
    if raw: headers['Content-Type'] = 'application/octet-stream'
    # Registrarse o entrar con una sesión abierta la rota (la borra): esas peticiones van sin la cookie del administrador.
    if anon: headers = {'Content-Type':'application/json'}
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
def fails(code, path, body=None, method=None, device=None, anon=False):
    """La petición debe fallar con ese texto (código HTTP o código de la API)."""
    try:
        req(path, body, method, device, anon=anon)
    except RuntimeError as e:
        return code in str(e)
    return False
def drops(path, device=None):
    """El servidor corta la conexión sin responder: la petición entra en pánico al leer un dato que no entiende."""
    try:
        req(path, device=device, raw=bool(device))
    except RuntimeError:
        return False
    except (ConnectionError, OSError):
        return True
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
def enroll(name, net):
    enroll_with(name, req(net+'/enrollment-tokens',{'ttl_secs':3600})['token'])
    ck(req(net+'/devices/'+s[name]+'/approve',{})['status']=='approved',name+' aprobado')
def config(name):
    return len(req('/api/v1/device/config',device=name,raw=True))>100
def N(key):
    return '/api/v1/networks/'+s[key]
policy={'src':{'kind':'any'},'dst':{'kind':'any'},'proto':0,'port_min':0,'port_max':0}
if stage=='N':
    s['org']=req('/api/v1/orgs',{'name':'rollback-org'})['id']
    root=cli('root-init','--out',str(p/'root.key')).splitlines()[-1]
    s['net']=req('/api/v1/orgs/'+s['org']+'/networks',{'name':'rollback-net','ipv4_cidr':'10.42.0.0/24','ipv6_cidr':'fd42::/64','root_anchor':root})['id']
    cli('issuer-cert','--root',str(p/'root.key'),'--issuer-key',os.environ['PROBE_ISSUER'],'--network-id',s['net'].replace('-',''),'--serial','1','--valid-days','30','--out',str(p/'issuer.cert'))
    req(N('net')+'/issuer-cert',(p/'issuer.cert').read_bytes(),'PUT',raw=True)
    for name in ('uno','dos'):
        enroll(name,N('net'))
    s['policy']=req(N('net')+'/policies',policy)['id']
    ck(config('uno'),'siembra: N emite configuración firmada')
    ck(fails('HTTP 404','/auth/local/options'),'N no tiene cuentas propias (404)')
org='/api/v1/orgs/'+s.get('org','')
if stage=='nuevo':
    row=req(N('net'))
    ck(row['root_mode']=='own' and row['ipv6_cidr']=='fd42::/64' and row['ipv4_pool'] is None,'N+1: la red migrada es propia, con su IPv6 y sin rango')
    ck(config('uno'),'N+1: emite configuración a un dispositivo anterior')
    m=req(org+'/networks',{'name':'gestionada'})
    s['managed']=m['id']
    ck(m['root_mode']=='managed' and m['has_issuer_cert'] and m['ipv6_cidr'],'N+1: red gestionada con certificado y doble pila')
    enroll('gest',N('managed'))
    ck(config('gest'),'N+1: emite configuración en la red gestionada')
    # La red solo IPv4 va en otra organización: así se mide por separado qué ve N en cada una.
    s['org4']=req('/api/v1/orgs',{'name':'rollback-solo-v4'})['id']
    v=req('/api/v1/orgs/'+s['org4']+'/networks',{'name':'solo-v4','ipv6':False,'ipv4_pool':{'start':'10.244.0.10','end':'10.244.0.20'}})
    s['v4']=v['id']
    ck(v['ipv6_cidr'] is None and v['ipv4_pool']=={'start':'10.244.0.10','end':'10.244.0.20'},'N+1: red solo IPv4 con rango de reparto')
    enroll('solo',N('v4'))
    ck(config('solo'),'N+1: emite configuración en la red solo IPv4')
    ck(req('/auth/local/options')=={'registration_open':True},'N+1: registro abierto en este ensayo')
    r=req('/auth/local/register',{'login':'local@example.invalid','password':os.environ['LOCAL_PASSWORD'],'display_name':'Local'},anon=True)
    ck(len(r['recovery_codes'])>0,'N+1: cuenta propia registrada (con organización personal)')
elif stage=='retroceso':
    ck(any(o['id']==s['org'] for o in req('/api/v1/orgs')),'organización legible')
    ck(req(N('net'))['name']=='rollback-net','red anterior legible')
    ck(len(req(N('net')+'/devices'))==2,'dispositivos de la red anterior legibles')
    ck(config('uno'),'N emite configuración firmada en la red anterior')
    enroll('tres',N('net'))
    ck(config('tres'),'N inscribe y sirve a un dispositivo nuevo en la red anterior')
    ck(len(req(org+'/networks'))==2,'N lista la organización con la red gestionada (la ve como una red más)')
    ck('root_mode' not in req(N('managed')),'N no sabe que la red es gestionada')
    ck(config('gest'),'N sigue sirviendo la red gestionada mientras su certificado valga (no lo renueva)')
    root=cli('root-init','--out',str(p/'root2.key')).splitlines()[-1]
    s['net2']=req(org+'/networks',{'name':'creada-por-n','ipv4_cidr':'10.43.0.0/24','ipv6_cidr':'fd43::/64','root_anchor':root})['id']
    ck(True,'N crea una red propia sobre el esquema nuevo')
    # Lo que N no sabe leer: `ipv6_cidr` nulo. No responde 500: la petición entra en pánico y se corta la conexión.
    ck(drops('/api/v1/orgs/'+s['org4']+'/networks'),'N NO lista la organización con una red solo IPv4 (corta la conexión)')
    ck(drops(N('v4')),'N NO lee la red solo IPv4 (corta la conexión)')
    ck(drops('/api/v1/device/config',device='solo'),'N NO emite configuración en la red solo IPv4 (corta la conexión)')
    ck(len(req(N('v4')+'/devices'))==1,'N sí lista los dispositivos de la red solo IPv4')
    ck(config('uno') and len(req(org+'/networks'))==3,'el proceso de N sigue vivo y sirve las demás redes tras esos cortes')
    ck(fails('HTTP 404','/auth/local/login',{'login':'local@example.invalid','password':os.environ['LOCAL_PASSWORD']},anon=True),'N: la cuenta propia no puede entrar (404)')
elif stage=='reupgrade':
    ck(len(req(N('net')+'/devices'))==3,'persistencia: tres dispositivos en la red anterior')
    ck(req(N('net2'))['root_mode']=='own' and req(N('net2'))['ipv4_pool'] is None,'N+1: la red que creó N es propia y sin rango')
    ck(req(N('managed'))['root_mode']=='managed' and config('gest'),'N+1: la red gestionada sigue gestionada y sirviendo')
    ck(req(N('v4'))['ipv6_cidr'] is None and config('solo'),'N+1: la red solo IPv4 vuelve a leerse y a servir')
    ck(len(req('/api/v1/orgs/'+s['org4']+'/networks'))==1,'N+1: la organización de la red solo IPv4 vuelve a listarse')
    r=req('/auth/local/login',{'login':'local@example.invalid','password':os.environ['LOCAL_PASSWORD']},anon=True)
    ck(True,'N+1: la cuenta propia vuelve a entrar')
state_path.write_text(json.dumps(s))
PY
probe() {
  PROBE_DIR=$TMP PROBE_BASE="http://127.0.0.1:$CP_PORT" PROBE_CLI=$TMP/bin/atenea PROBE_ISSUER=$TMP/issuer.key \
    UPGRADE_SESSION=$SESSION_TOKEN UPGRADE_CSRF=$CSRF_TOKEN LOCAL_PASSWORD=$LOCAL_PASSWORD python3 "$TMP/probe.py" "$1"
}
start "$TMP/bin/n"
probe N
stop
cat > "$TMP/fingerprint.sql" <<'SQL'
SELECT md5(string_agg(v,E'\n' ORDER BY v)) FROM (
 SELECT to_jsonb(t)::text v FROM devices t
 UNION ALL SELECT to_jsonb(t)::text FROM enrollment_tokens t
 UNION ALL SELECT (to_jsonb(t)-'personal_org_done')::text FROM users t
 UNION ALL SELECT (to_jsonb(t)-'personal')::text FROM organizations t
 UNION ALL SELECT (to_jsonb(t)-'root_mode'-'root_wrapped'-'ipv4_pool_start'-'ipv4_pool_end')::text FROM networks t
 UNION ALL SELECT to_jsonb(t)::text FROM access_policies t
 UNION ALL SELECT to_jsonb(t)::text FROM memberships t
 UNION ALL SELECT to_jsonb(t)::text FROM ip_allocations t
) fixture;
SQL
BASELINE=$(sql < "$TMP/fingerprint.sql")
printf '\nFASE 2: upgrade N+1, 0015 a 0018, red gestionada, red solo IPv4 y cuenta propia\n'
migrate "$TMP/bin/n1"
check_versions 1,2,3,4,5,6,7,8,9,10,11,12,13,14,15,16,17,18
ok 'historial 1–18 exitoso'
[ "$HISTORY" = "$(checksums 14)" ]
ok 'checksums 1–14 intactos'
HISTORY18=$(checksums 18)
[ "$BASELINE" = "$(sql < "$TMP/fingerprint.sql")" ]; ok 'valores de toda la siembra intactos al migrar'
expect_sql "SELECT count(*) FROM networks WHERE root_mode<>'own' OR root_wrapped IS NOT NULL OR ipv4_pool_start IS NOT NULL" 0 'las redes migradas son propias y sin rango'
expect_sql "SELECT is_nullable FROM information_schema.columns WHERE table_name='networks' AND column_name='ipv6_cidr'" YES 'ipv6_cidr es nulable'
start "$TMP/bin/n1"
probe nuevo
stop
expect_sql "SELECT count(*) FROM networks WHERE root_mode='managed' AND root_wrapped IS NOT NULL" 2 'dos redes gestionadas con la raíz cifrada'
expect_sql "SELECT count(*) FROM networks WHERE ipv6_cidr IS NULL" 1 'una red solo IPv4'
expect_sql "SELECT count(*) FROM local_credentials" 1 'una cuenta propia'
expect_sql "SELECT count(*) FROM organizations WHERE personal" 1 'una organización personal'
SERIALS=$(sql -c "SELECT string_agg(encode(issuer_cert,'hex'),',' ORDER BY id) FROM networks WHERE root_mode='managed'")
printf '\nFASE 3: retroceso por binario, arranque normal N sin --migrate\n'
start "$TMP/bin/n"
probe retroceso
stop
check_versions 1,2,3,4,5,6,7,8,9,10,11,12,13,14,15,16,17,18
ok 'historial 1–18 intacto tras arrancar N'
[ "$SERIALS" = "$(sql -c "SELECT string_agg(encode(issuer_cert,'hex'),',' ORDER BY id) FROM networks WHERE root_mode='managed'")" ]; ok 'N no tocó los certificados de las redes gestionadas'
expect_sql "SELECT root_mode||':'||(root_wrapped IS NULL)::text FROM networks WHERE name='creada-por-n'" 'own:true' 'la red que creó N quedó como propia por los valores por defecto'
expect_sql "SELECT count(*) FROM organizations WHERE personal" 1 'la organización personal sigue marcada'
printf '\nFASE 4: negativo N --migrate sobre 0018\n'
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
[ "$NEGATIVE_RC" -ne 0 ]; ok 'N --migrate rechaza BD con 0018 (código distinto de 0)'
[ "$BEFORE" = "$(dump_hash)" ]; ok 'base completa e historial no cambiaron tras el negativo'
printf '\nFASE 5: re-upgrade N+1 idempotente y persistencia\n'
BEFORE=$(dump_hash)
migrate "$TMP/bin/n1"
[ "$BEFORE" = "$(dump_hash)" ]; ok 'migración N+1 idempotente, sin cambios en la base'
check_versions 1,2,3,4,5,6,7,8,9,10,11,12,13,14,15,16,17,18
ok 'historial final 1–18 exitoso'
[ "$HISTORY18" = "$(checksums 18)" ]; ok 'checksums 1–18 iguales tras la nueva subida'
start "$TMP/bin/n1"
probe reupgrade
stop
printf 'PASS retroceso 0018 completo en %s s; solo control plane/PostgreSQL sintéticos\n' "$((SECONDS-START))"
