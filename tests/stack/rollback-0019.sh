#!/usr/bin/env bash
# Copyright 2026 Tutitoos
# SPDX-License-Identifier: Apache-2.0
# Retroceso v0.1.35 (migraciones 1–18) sobre la 0019 (conexiones automáticas por aprobación, ADR-031). No ejecuta down
# migrations: el binario anterior debe funcionar con el esquema ampliado. Deja medido qué NO sabe hacer el binario
# anterior con lo que solo existe desde la 0.1.36: no emite las conexiones automáticas (esos pares desaparecen de la
# configuración firmada), no registra el orden de aprobación (lo que aprueba queda con orden 0, igual que lo anterior a la
# migración, y al volver a subir se conecta con los aprobados después con el ajuste activo) y no entiende `?config_v3` ni
# el campo `config_v3` (los ignora sin romper la petición).
#
# N es el tag v0.1.35 y N+1 el tag v0.1.36 (la primera versión con la 0019); `ATENEA_NEXT_REF` admite cualquier revisión.
# Como rollback-0018.sh, compila los dos control plane en el anfitrión y usa solo PostgreSQL 17 en Docker, con datos
# sintéticos y los binarios reales. Requiere docker, cargo, python3 y un openssl con Ed25519.
set -euo pipefail
ROOT=$(git -C "$(dirname "$0")" rev-parse --show-toplevel)
cd "$ROOT"
umask 077
START=$SECONDS
RUN="atenea-rollback-0019-$$"
PREVIOUS_TAG=${ATENEA_PREVIOUS_TAG:-v0.1.35}
NEXT_REF=${ATENEA_NEXT_REF:-v0.1.36}
TMP=$(mktemp -d "${TMPDIR:-/tmp}/atenea-rollback-0019.XXXXXX")
TMP=$(cd "$TMP" && pwd -P)
PG=$RUN-pg
PORT=${ATENEA_ROLLBACK_PG_PORT:-54178}
CP_PORT=${ATENEA_ROLLBACK_CP_PORT:-54179}
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
[ "$#" -eq 18 ]
set -- "$TMP/next"/apps/control-plane/migrations/*.sql
[ "$#" -eq 19 ]
test -f "$TMP/next/apps/control-plane/migrations/0019_auto_connect.sql"
printf 'N %s commit=%s; N+1 %s commit=%s\n' "$PREVIOUS_TAG" "$PREVIOUS" "$NEXT_REF" "$NEXT"

# Binarios: cada revisión se compila con su propio árbol; la CLI es la de N+1 (device-init, root-init, issuer-cert).
mkdir -p "$TMP/bin"
# Un directorio de compilación por revisión: con uno compartido, cargo toma como fresco el crate de N+1 (sus ficheros son
# más antiguos que el artefacto que acaba de compilar N) y enlaza N+1 contra el código de N.
build() {
  local out=$TARGET/rollback-0019/$(basename "$1")
  (cd "$1" && CARGO_TARGET_DIR=$out nice -n 19 cargo build --locked -j 2 -p atenea-control-plane "${@:3}")
  cp "$out/debug/atenea-control-plane" "$2"
}
build "$TMP/previous" "$TMP/bin/n"
build "$TMP/next" "$TMP/bin/n1" -p atenea-cli
cp "$TARGET/rollback-0019/next/debug/atenea" "$TMP/bin/atenea"
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

printf '\nFASE 1: N %s, migraciones 1–18 y siembra\n' "$PREVIOUS_TAG"
migrate "$TMP/bin/n"
check_versions 1,2,3,4,5,6,7,8,9,10,11,12,13,14,15,16,17,18
ok 'N: historial 1–18 exitoso'
HISTORY=$(checksums 18)
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
def init(name):
    cli('device-init','--dir',str(p/name))
def enroll(name, net, extra=None):
    init(name)
    token=req(net+'/enrollment-tokens',{'ttl_secs':3600})['token']
    body={'token':token,'bundle':(p/name/'bundle.bin').read_bytes().hex(),'name':'lab-'+name}
    body.update(extra or {})
    d=req('/api/v1/enroll',body)
    s[name]=d['device_id']
    ck(d['status']=='pending',name+' inscrito pendiente por API')
    ck(req(net+'/devices/'+s[name]+'/approve',{})['status']=='approved',name+' aprobado')
def config(name):
    return req('/api/v1/device/config',device=name,raw=True)
def peers_in(b):
    """Pares de una configuración firmada (AOCF v1 + certificado 178 + longitud 4 + cuerpo + firma 64)."""
    body=b[187:-64]
    o=133
    for with_len in (False, True):  # tunnel_addrs (direcciones) y network_prefixes (dirección + longitud)
        n=body[o]; o+=1
        for _i in range(n):
            o+=1+(4 if body[o]==4 else 16)+(1 if with_len else 0)
    return int.from_bytes(body[o:o+2],'big')
def N(key):
    return '/api/v1/networks/'+s[key]
policy={'src':{'kind':'any'},'dst':{'kind':'any'},'proto':0,'port_min':0,'port_max':0}
if stage=='N':
    s['org']=req('/api/v1/orgs',{'name':'rollback-org'})['id']
    root=cli('root-init','--out',str(p/'root.key')).splitlines()[-1]
    s['vieja']=req('/api/v1/orgs/'+s['org']+'/networks',{'name':'vieja','ipv4_cidr':'10.42.0.0/24','ipv6_cidr':'fd42::/64','root_anchor':root})['id']
    cli('issuer-cert','--root',str(p/'root.key'),'--issuer-key',os.environ['PROBE_ISSUER'],'--network-id',s['vieja'].replace('-',''),'--serial','1','--valid-days','30','--out',str(p/'issuer.cert'))
    req(N('vieja')+'/issuer-cert',(p/'issuer.cert').read_bytes(),'PUT',raw=True)
    for name in ('uno','dos'):
        enroll(name,N('vieja'))
    s['policy']=req(N('vieja')+'/policies',policy)['id']
    ck(peers_in(config('uno'))==1,'siembra: N emite a uno su par por la política manual')
    s['root_hex']=root
org='/api/v1/orgs/'+s.get('org','')
if stage=='nuevo':
    row=req(N('vieja'))
    ck(row['auto_connect'] is False,'N+1: la red migrada conserva el acceso manual (auto_connect falso)')
    ck(peers_in(config('uno'))==1,'N+1: la red migrada emite lo mismo que N (un par por la política manual)')
    root=cli('root-init','--out',str(p/'root2.key')).splitlines()[-1]
    s['auto']=req(org+'/networks',{'name':'auto','ipv4_cidr':'10.44.0.0/24','ipv6_cidr':'fd44::/64','root_anchor':root})['id']
    ck(req(N('auto'))['auto_connect'] is True,'N+1: una red nueva nace con conexiones automáticas')
    cli('issuer-cert','--root',str(p/'root2.key'),'--issuer-key',os.environ['PROBE_ISSUER'],'--network-id',s['auto'].replace('-',''),'--serial','1','--valid-days','30','--out',str(p/'issuer2.cert'))
    req(N('auto')+'/issuer-cert',(p/'issuer2.cert').read_bytes(),'PUT',raw=True)
    for name in ('a','b','c'):
        enroll(name,N('auto'),{'config_v3':True})
    ck(sorted(req(N('auto')+'/devices/'+s['a']+'/auto-peers'))==sorted([s['b'],s['c']]),'N+1: a se conecta automáticamente con b y c (aprobados después)')
    ck(peers_in(config('a'))==2 and peers_in(config('c'))==2,'N+1: la configuración firmada de a y c lleva 2 pares sin ninguna política')
    ck(req('/api/v1/networks/'+s['auto']+'/policies')==[],'N+1: no hay ninguna política por pareja')
    ck(req('/api/v1/device/status?config_v3=true',{'paths':[]},device='a') is None,'N+1: el informe con ?config_v3=true se acepta')
    req(N('auto'),{'auto_connect':False},'PATCH')
    enroll('d',N('auto'))
    ck(peers_in(config('d'))==0 and s['d'] not in req(N('auto')+'/devices/'+s['a']+'/auto-peers'),'N+1: con el ajuste apagado d no se conecta con nadie')
    req(N('auto'),{'auto_connect':True},'PATCH')
elif stage=='retroceso':
    ck(len(req(N('vieja')+'/devices'))==2,'N lee los dispositivos de la red anterior')
    ck(peers_in(config('uno'))==1,'N emite en la red anterior lo mismo que antes')
    row=req(N('auto'))
    ck('auto_connect' not in row,'N no conoce el ajuste de conexiones automáticas')
    ck(len(req(N('auto')+'/devices'))==4,'N lista los cuatro dispositivos de la red con conexiones automáticas')
    ck(peers_in(config('a'))==0 and peers_in(config('b'))==0,'N NO emite las conexiones automáticas: a y b se quedan sin pares')
    ck(req('/api/v1/device/status?config_v3=true',{'paths':[]},device='b') is None,'N ignora ?config_v3=true sin romper el informe firmado')
    enroll('e',N('auto'),{'config_v3':True})
    ck(True,'N acepta el campo config_v3 en el alta y aprueba a e (sin orden de aprobación)')
    ck(peers_in(config('e'))==0,'N: e queda sin pares')
    ck(peers_in(config('uno'))==1,'el proceso de N sigue vivo y sirve la red anterior')
elif stage=='reupgrade':
    ck(len(req(N('auto')+'/devices'))==5,'persistencia: cinco dispositivos en la red con conexiones automáticas')
    ck(req(N('auto'))['auto_connect'] is True,'N+1: el ajuste de la red sigue activo')
    got=sorted(req(N('auto')+'/devices/'+s['a']+'/auto-peers'))
    print('MEDIDO a auto-peers:', len(got), 'e incluido:', s['e'] in got, 'pares de a:', peers_in(config('a')), 'pares de e:', peers_in(config('e')), flush=True)
    ck(set(got)>={s['b'],s['c']},'N+1: las conexiones automáticas de a con b y c vuelven solas')
    # LÍMITE CONOCIDO: el binario anterior no registra el orden de aprobación (queda en 0, como los dispositivos
    # anteriores a la migración), así que lo que aprobó mientras duró el retroceso cuenta como «aprobado antes» de todo y
    # se conecta con los aprobados después con el ajuste activo, aunque el ajuste estuviera apagado al aprobarlo.
    ck(s['e'] in got and peers_in(config('e'))==3,'N+1: e, aprobado por N, queda conectado con a, b y c (orden 0 = anterior a todo)')
    ck(peers_in(config('uno'))==1,'N+1: la red anterior emite lo mismo')
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
 SELECT (to_jsonb(t)-'approval_order'-'auto_connect_on_approval'-'config_v3')::text v FROM devices t
 UNION ALL SELECT to_jsonb(t)::text FROM enrollment_tokens t
 UNION ALL SELECT to_jsonb(t)::text FROM users t
 UNION ALL SELECT to_jsonb(t)::text FROM organizations t
 UNION ALL SELECT (to_jsonb(t)-'auto_connect')::text FROM networks t
 UNION ALL SELECT to_jsonb(t)::text FROM access_policies t
 UNION ALL SELECT to_jsonb(t)::text FROM memberships t
 UNION ALL SELECT to_jsonb(t)::text FROM ip_allocations t
) fixture;
SQL
BASELINE=$(sql < "$TMP/fingerprint.sql")
printf '\nFASE 2: upgrade N+1, 0019, red con conexiones automáticas y excepciones\n'
migrate "$TMP/bin/n1"
check_versions 1,2,3,4,5,6,7,8,9,10,11,12,13,14,15,16,17,18,19
ok 'historial 1–19 exitoso'
[ "$HISTORY" = "$(checksums 18)" ]
ok 'checksums 1–18 intactos'
[ "$BASELINE" = "$(sql < "$TMP/fingerprint.sql")" ]; ok 'valores de toda la siembra intactos al migrar'
expect_sql "SELECT count(*) FROM networks WHERE auto_connect" 0 'la red migrada empieza con acceso manual'
expect_sql "SELECT count(*) FROM devices WHERE approval_order<>0 OR auto_connect_on_approval OR config_v3" 0 'los dispositivos migrados no heredan orden, ajuste ni capacidad'
start "$TMP/bin/n1"
probe nuevo
stop
expect_sql "SELECT count(*) FROM networks WHERE auto_connect" 1 'una red con conexiones automáticas'
expect_sql "SELECT count(*) FROM access_policies WHERE network_id=(SELECT id FROM networks WHERE name='auto')" 0 'ninguna política guardada para esa red'
printf '\nFASE 3: retroceso por binario, arranque normal N sin --migrate\n'
start "$TMP/bin/n"
probe retroceso
stop
check_versions 1,2,3,4,5,6,7,8,9,10,11,12,13,14,15,16,17,18,19
ok 'historial 1–19 intacto tras arrancar N'
expect_sql "SELECT count(*) FROM networks WHERE auto_connect" 1 'N no tocó el ajuste de la red'
expect_sql "SELECT approval_order||':'||auto_connect_on_approval FROM devices WHERE name='lab-e'" '0:false' 'el dispositivo aprobado por N quedó con orden 0 y sin ajuste'
printf '\nFASE 4: negativo N --migrate sobre 0019\n'
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
[ "$NEGATIVE_RC" -ne 0 ]; ok 'N --migrate rechaza BD con 0019 (código distinto de 0)'
[ "$BEFORE" = "$(dump_hash)" ]; ok 'base completa e historial no cambiaron tras el negativo'
printf '\nFASE 5: re-upgrade N+1 idempotente y persistencia\n'
BEFORE=$(dump_hash)
migrate "$TMP/bin/n1"
[ "$BEFORE" = "$(dump_hash)" ]; ok 'migración N+1 idempotente, sin cambios en la base'
check_versions 1,2,3,4,5,6,7,8,9,10,11,12,13,14,15,16,17,18,19
ok 'historial final 1–19 exitoso'
start "$TMP/bin/n1"
probe reupgrade
stop
printf 'PASS retroceso 0019 completo en %s s; solo control plane/PostgreSQL sintéticos\n' "$((SECONDS-START))"
