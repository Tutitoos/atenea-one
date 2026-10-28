#!/usr/bin/env bash
# Copyright 2026 Tutitoos
# SPDX-License-Identifier: Apache-2.0
# Retroceso v0.1.5 sobre 0008 con escrituras nuevas, sin down migrations.
# No ejecuta down migrations: el binario anterior debe funcionar con el esquema ampliado.
set -euo pipefail
ROOT=$(git -C "$(dirname "$0")" rev-parse --show-toplevel)
cd "$ROOT"
umask 077
START=$SECONDS
RUN="atenea-stack-upgrade-$$"
TMP=$(mktemp -d "${TMPDIR:-/tmp}/atenea-stack-upgrade.XXXXXX")
# Docker Desktop debe poder montar el worktree (macOS resuelve /var -> /private/var).
TMP=$(cd "$TMP" && pwd -P)
NET=$RUN
PG=$RUN-pg
CP=$RUN-cp
OLD_IMAGE=$RUN-n
NEW_IMAGE=$RUN-next
POSTGRES_IMAGE=postgres:17@sha256:b8faeff7712663c2773a190181e1246e37e364b887611c04658c28b44e2408fe
PROBE_IMAGE=${UPGRADE_PROBE_IMAGE:-atenea-panel-lab:latest}
cleanup() {
  local rc=$?
  trap - EXIT
  local passed=0
  if [ -f "$TMP/checks" ]; then passed=$(wc -l < "$TMP/checks" | tr -d " "); fi
  printf "%s comprobaciones; pasan %s; fallan %s\n" "$((passed + (rc != 0)))" "$passed" "$((rc != 0))"
  docker rm -fv "$CP" "$PG" >/dev/null 2>&1 || true
  docker volume rm "$RUN-key" >/dev/null 2>&1 || true
  docker network rm "$NET" >/dev/null 2>&1 || true
  docker image rm "$OLD_IMAGE" "$NEW_IMAGE" >/dev/null 2>&1 || true
  if [ -d "$TMP/previous" ]; then git worktree remove --force "$TMP/previous"; fi
  rm -rf "$TMP"
  exit "$rc"
}
trap cleanup EXIT
trap 'exit 130' INT
trap 'exit 143' TERM

docker info >/dev/null
PREVIOUS=$(git rev-parse v0.1.5^{commit})
git worktree add --detach "$TMP/previous" "$PREVIOUS"
set -- "$TMP/previous"/apps/control-plane/migrations/*.sql
[ "$#" -eq 7 ]
test -f apps/control-plane/migrations/0008_device_role.sql
printf 'N commit=%s; N+1 working-tree HEAD=%s\n' "$PREVIOUS" "$(git rev-parse HEAD)"

# Compilación sin red con las cachés de laboratorio existentes; rustup no interviene.
BUILDER_IMAGE=${UPGRADE_BUILDER_IMAGE:-atenea-lab:latest}
RUNTIME_IMAGE=${UPGRADE_RUNTIME_IMAGE:-gcr.io/distroless/cc-debian13:nonroot@sha256:3ee00291cf386ec52a552e6a858d555debd74776eb4bfa53fa3df2cd6677515e}
docker image inspect "$BUILDER_IMAGE" "$RUNTIME_IMAGE" "$POSTGRES_IMAGE" "$PROBE_IMAGE" >/dev/null
cat > "$TMP/Dockerfile" <<'DOCKERFILE'
ARG RUNTIME_IMAGE=gcr.io/distroless/cc-debian13:nonroot@sha256:3ee00291cf386ec52a552e6a858d555debd74776eb4bfa53fa3df2cd6677515e
FROM ${RUNTIME_IMAGE}
COPY atenea-control-plane /usr/local/bin/atenea-control-plane
USER 65532:65532
ENTRYPOINT ["/usr/local/bin/atenea-control-plane"]
DOCKERFILE
build() {
  local source=$1 image=$2
  docker run --rm --network none -e RUSTUP_TOOLCHAIN=1.99.1 -e CARGO_BUILD_JOBS=2 -e CARGO_TARGET_DIR=/cache/target \
    -v "$source:/src:ro" -v "$TMP:/out" \
    -v atenea-cargo-registry:/usr/local/cargo/registry -v atenea-lab-target:/cache \
    "$BUILDER_IMAGE" bash -euc '
      mkdir -p /tmp/build/apps
      cp /src/Cargo.toml /src/Cargo.lock /tmp/build/
      cp -a /src/crates /tmp/build/
      for app in agent cli control-plane coordinator relay; do cp -a "/src/apps/$app" /tmp/build/apps/; done
      cd /tmp/build
      # Cargo usa mtimes: N y N+1 no deben reutilizar artefactos locales de la otra revisión.
      find crates apps -name "*.rs" -exec touch {} +
      cargo build --release --locked --offline -p atenea-control-plane
      cp /cache/target/release/atenea-control-plane /out/atenea-control-plane
    '
  docker build --network none --pull=false --progress=plain \
    --build-arg "RUNTIME_IMAGE=$RUNTIME_IMAGE" -f "$TMP/Dockerfile" -t "$image" "$TMP"
}
build "$TMP/previous" "$OLD_IMAGE"
build "$ROOT" "$NEW_IMAGE"
OLD_DIGEST=$(docker image inspect --format '{{.Id}}' "$OLD_IMAGE")
NEW_DIGEST=$(docker image inspect --format '{{.Id}}' "$NEW_IMAGE")
[ "$OLD_DIGEST" != "$NEW_DIGEST" ]
printf 'N digest=%s\nN+1 digest=%s\n' "$OLD_DIGEST" "$NEW_DIGEST"

# Secretos exclusivos de esta ejecución: RNG del contenedor, nunca fixtures ni salida.
docker run --rm --entrypoint sh "$POSTGRES_IMAGE" -c '
  for key in PG_PASSWORD APP_PASSWORD SESSION_TOKEN CSRF_TOKEN ENROLLMENT_TOKEN ISSUER_SEED; do
    printf "%s=" "$key"
    od -An -N32 -tx1 /dev/urandom | tr -d " \\n"
    printf "\\n"
  done
' > "$TMP/secrets.env"
source "$TMP/secrets.env"
docker network create "$NET" >/dev/null
docker run -d --name "$PG" --network "$NET" --network-alias postgres \
  -e "POSTGRES_PASSWORD=$PG_PASSWORD" -e POSTGRES_DB=atenea "$POSTGRES_IMAGE" >/dev/null
for ((i=0; i<60; i++)); do
  if docker exec "$PG" pg_isready -U postgres -d atenea >/dev/null 2>&1; then break; fi
  sleep 1
done
docker exec "$PG" pg_isready -U postgres -d atenea >/dev/null
# El volumen es temporal; copiar dentro permite conservar 0600 bajo UID 65532.
docker volume create "$RUN-key" >/dev/null
printf '%s\n' "$ISSUER_SEED" | docker run --rm -i -v "$RUN-key:/key" --entrypoint sh "$POSTGRES_IMAGE" -c \
  'cat > /key/issuer.key; chown 65532:65532 /key/issuer.key; chmod 600 /key/issuer.key'

sql() { docker exec -i "$PG" psql -X -U postgres -d atenea -v ON_ERROR_STOP=1 -At "$@"; }
cp_args=(--network "$NET" -v "$RUN-key:/key:ro"
  -e RUST_LOG=atenea_control_plane=info
  -e ATENEA_CP_ISSUER_KEY_FILE=/key/issuer.key
  -e ATENEA_CP_LISTEN=0.0.0.0:8080
  # Metadato HTTPS válido para códigos; las peticiones del laboratorio son HTTP aislado.
  -e ATENEA_CP_PUBLIC_URL=https://control.example.invalid)
migrate() {
  docker run --rm "${cp_args[@]}" \
    -e "ATENEA_CP_DATABASE_URL=postgres://postgres:$PG_PASSWORD@postgres/atenea" \
    -e ATENEA_CP_APPLICATION_ROLE=atenea_app "$1" --migrate
}
start() {
  docker run -d --name "$CP" --network-alias control-plane "${cp_args[@]}" \
    -e "ATENEA_CP_DATABASE_URL=postgres://atenea_app:$APP_PASSWORD@postgres/atenea" "$1" >/dev/null
  [ "$(docker inspect --format '{{.Image}}' "$CP")" = "$1" ]
}
stop() { docker rm -f "$CP" >/dev/null; }
check_versions() {
  local actual
  actual=$(sql -c 'SELECT string_agg(version::text,'"'"','"'"' ORDER BY version) FROM _sqlx_migrations WHERE success')
  [ "$actual" = "$1" ] || { printf 'Migraciones inesperadas: %s\n' "$actual" >&2; return 1; }
}
ok() { printf 'PASS %s\n' "$1"; printf '.\n' >> "$TMP/checks"; }
expect_sql() { [ "$(sql -c "$1")" = "$2" ]; ok "$3"; }
printf '\nFASE 1: N v0.1.5, migraciones 1–7 y siembra\n'
migrate "$OLD_DIGEST"
check_versions 1,2,3,4,5,6,7
ok 'N: historial 1–7 exitoso'
HISTORY=$(sql -c "SELECT string_agg(version::text||':'||encode(checksum,'hex'),',' ORDER BY version) FROM _sqlx_migrations WHERE version<=7")
sql -v app_password="$APP_PASSWORD" -v session="$SESSION_TOKEN" -v csrf="$CSRF_TOKEN" <<'SQL'
ALTER ROLE atenea_app PASSWORD :'app_password';
INSERT INTO users(id,oidc_issuer,oidc_subject,email,display_name) VALUES
('00000000-0000-0000-0000-000000000001','https://rollback.invalid','owner','owner@example.invalid','Propietario sintético'),
('00000000-0000-0000-0000-000000000007','https://rollback.invalid','member','member@example.invalid','Miembro sintético');
INSERT INTO sessions(id,user_id,csrf_hash,csrf_token,expires_at) VALUES (sha256(:'session'::bytea),'00000000-0000-0000-0000-000000000001',sha256(:'csrf'::bytea),:'csrf',now()+interval '8 hours');
SQL
# El cliente y OpenSSL ya están en las cachés existentes; no se instalan dependencias.
cat > "$TMP/probe.py" <<'PY'
import hashlib, json, os, pathlib, subprocess, sys, time, urllib.error, urllib.request
stage = sys.argv[1]
p = pathlib.Path('/run-data')
state_path = p/'state.json'
s = json.loads(state_path.read_text()) if state_path.exists() else {}
base = 'http://control-plane:8080'
def ck(value, label):
    if not value:
        raise AssertionError(label)
    with (p/'checks').open('a') as out: out.write('.\n')
    print('PASS '+stage+': '+label, flush=True)
def cli(*args):
    return subprocess.check_output(['/cache/target/release/atenea', *args], stderr=subprocess.PIPE).decode().strip()
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
        # No volcar cuerpos que pudieran contener tokens o material de sesión.
        raise RuntimeError(f'{stage} {method} {path}: HTTP {e.code}') from None
for attempt in range(60):
    try:
        req('/healthz'); break
    except Exception:
        if attempt==59: raise
        time.sleep(1)
def enroll(name, role=None, approve=False):
    cli('device-init','--dir',str(p/name))
    token=req(net+'/enrollment-tokens',dict(ttl_secs=3600, **({'role':role} if role else {})))
    body={'token':token['token'],'bundle':(p/name/'bundle.bin').read_bytes().hex(),'name':'lab-'+name}
    d=req('/api/v1/enroll',body)
    s[name]=d['device_id']
    ck(d['status']=='pending',name+' inscrito pendiente por API')
    if approve:
        ck(req(net+'/devices/'+s[name]+'/approve',{})['status']=='approved',name+' aprobado')
policy={'src':{'kind':'any'},'dst':{'kind':'any'},'proto':0,'port_min':0,'port_max':0}
if stage=='N':
    s['org']=req('/api/v1/orgs',{'name':'rollback-org'})['id']
    root=cli('root-init','--out',str(p/'root.key')).splitlines()[-1]
    s['root']=root
    s['net']=req('/api/v1/orgs/'+s['org']+'/networks',{'name':'rollback-net','ipv4_cidr':'10.42.0.0/24','ipv6_cidr':'fd42::/64','root_anchor':root})['id']
net='/api/v1/networks/'+s['net']
org='/api/v1/orgs/'+s['org']
if stage=='N':
    cli('issuer-cert','--root',str(p/'root.key'),'--issuer-key',str(p/'issuer.key'),'--network-id',s['net'].replace('-',''),'--serial','1','--valid-days','30','--out',str(p/'issuer.cert'))
    req(net+'/issuer-cert',(p/'issuer.cert').read_bytes(),'PUT',raw=True)
    enroll('approved',approve=True)
    enroll('pending')
    req(net+'/enrollment-tokens',{'ttl_secs':3600})
    s['policy']=req(net+'/policies',policy)['id']
    req('/api/v1/device/routes',{'routes':['10.43.0.0/24']},device='approved')
    route=req(net+'/routes')[0]
    s['route']=route['id']
    req(net+'/routes/'+s['route']+'/approve',{'snat':False})
    ck(any(d['id']==s['pending'] and d['status']=='pending' for d in req(net+'/devices')),'siembra aprobada, pendiente, token libre, política y ruta')
elif stage=='nuevo':
    enroll('server',role='server')
    ck(any(d['id']==s['server'] and d['role']=='server' for d in req(net+'/devices')),'token server propaga rol al dispositivo pendiente')
    ck(req(net+'/devices/'+s['approved'],{'role':'server'},'PATCH') is not None,'PATCH administrativo de rol')
    req(org,{'name':'rollback-org-renombrada'},'PATCH')
    req(net,{'name':'rollback-net-renombrada'},'PATCH')
    req(org+'/members',{'email':'member@example.invalid','role':'member'})
    req(org+'/members',{'email':'member@example.invalid','role':'admin'})
    members=req(org+'/members')
    ck(any(m['email']=='member@example.invalid' and m['role']=='admin' for m in members),'miembro por correo SQL sintético y cambio de rol')
    ck(set(req('/api/v1/server'))=={'control_plane','server_time'},'/server solo control plane y hora')
    ck(req(net)['issuer_cert_not_after'] is not None,'issuer_cert_not_after derivado del certificado')
    req(net+'/devices/'+s['pending']+'/approve',{})
    peers=req('/api/v1/device/peers',device='pending')
    # El server pendiente aún no es par: se aprueba con N en el retroceso.
    ck(any(d['device_id']==s['approved'] and d['role']=='server' for d in peers),'lectura nueva peers no vacía con role server')
elif stage=='retroceso':
    ck(any(o['id']==s['org'] and o['name']=='rollback-org-renombrada' for o in req('/api/v1/orgs')),'organización renombrada legible')
    ck(any(n['id']==s['net'] and n['name']=='rollback-net-renombrada' for n in req(org+'/networks')),'lista de redes renombradas legible')
    ck(req(net)['name']=='rollback-net-renombrada','detalle de red renombrada legible')
    devices=req(net+'/devices')
    ck(any(d['id']==s['server'] and 'role' not in d for d in devices),'N ve server como dispositivo normal')
    ck(any(d['id']==s['policy'] for d in req(net+'/policies')),'política conservada')
    ck(any(r['id']==s['route'] for r in req(net+'/routes')),'ruta conservada')
    events=req(net+'/audit')
    ck(any(e['action']=='device.role' for e in events),'auditoría device.role legible')
    ck(any(e['action']=='network.rename' for e in events),'auditoría network.rename legible')
    # member.set es de organización: se contrasta por SQL fuera del cliente.
    ck(req(net+'/devices/'+s['server']+'/approve',{})['status']=='approved','N aprueba server sin tocar role')
    ck(len(req('/api/v1/device/config',device='server',raw=True))>100,'N emite configuración a server con petición firmada')
    ck(any(d['device_id']==s['server'] for d in req('/api/v1/device/peers',device='approved')),'N lista server en peers con firma')
    ck(req(net+'/devices/'+s['server']+'/revoke',{}) is not None,'N revoca server')
    new_org=req('/api/v1/orgs',{'name':'escritura-retroceso'})
    s['rollback_org']=new_org['id']
    s['rollback_net']=req('/api/v1/orgs/'+new_org['id']+'/networks',{'name':'red-retroceso','ipv4_cidr':'10.44.0.0/24','ipv6_cidr':'fd44::/64','root_anchor':s['root']})['id']
    enroll('rollback',approve=True)
    ck(len(req('/api/v1/device/config',device='rollback',raw=True))>100,'N emite configuración firmada a dispositivo nuevo')
    ck(isinstance(req('/api/v1/device/peers',device='rollback'),list),'N peers firmado de dispositivo nuevo')
    temporary=req(net+'/policies',dict(policy,proto=6,port_min=443,port_max=443))
    req(net+'/policies/'+temporary['id'],method='DELETE')
    ck(not any(d['id']==temporary['id'] for d in req(net+'/policies')),'N crea y borra política')
    ck(any(o['id']==s['rollback_org'] for o in req('/api/v1/orgs')),'N crea organización y red')
elif stage=='reupgrade':
    devices=req(net+'/devices')
    for name in ('approved','pending','server','rollback'):
        expected='server' if name in ('approved','server') else 'device'
        ck(any(d['id']==s[name] and d['role']==expected for d in devices),'persistencia dispositivo '+name+' con rol '+expected)
    ck(any(d['id']==s['server'] and d['status']=='revoked' for d in devices),'revocación server conservada')
    ck(any(o['id']==s['rollback_org'] for o in req('/api/v1/orgs')),'organización escrita por N conservada')
    ck(any(n['id']==s['rollback_net'] for n in req('/api/v1/orgs/'+s['rollback_org']+'/networks')),'red escrita por N conservada')
    ck(req(net)['name']=='rollback-net-renombrada','renombrado N+1 conservado')
    ck(any(m['email']=='member@example.invalid' and m['role']=='admin' for m in req(org+'/members')),'membresía N+1 conservada')
    ck(any(r['id']==s['route'] for r in req(net+'/routes')),'ruta de N conservada')
    ck(any(d['id']==s['policy'] for d in req(net+'/policies')),'política de N conservada')
state_path.write_text(json.dumps(s))
PY
probe() {
  docker run --rm --network "$NET" -e "UPGRADE_SESSION=$SESSION_TOKEN" -e "UPGRADE_CSRF=$CSRF_TOKEN" \
    -v "$TMP:/run-data" -v atenea-lab-target:/cache:ro "$PROBE_IMAGE" python3 /run-data/probe.py "$1"
}
printf '%s\n' "$ISSUER_SEED" > "$TMP/issuer.key"
start "$OLD_DIGEST"
probe N
stop
# Excluye campos role (aún ausentes en N), sin actividad de muestreadores ni sesiones.
cat > "$TMP/fingerprint.sql" <<'SQL'
SELECT md5(string_agg(v,E'\n' ORDER BY v)) FROM (
 SELECT jsonb_strip_nulls(to_jsonb(t)-'role')::text v FROM devices t
 UNION ALL SELECT (to_jsonb(t)-'role')::text FROM enrollment_tokens t
 UNION ALL SELECT to_jsonb(t)::text FROM organizations t
 UNION ALL SELECT to_jsonb(t)::text FROM networks t
 UNION ALL SELECT to_jsonb(t)::text FROM access_policies t
 UNION ALL SELECT to_jsonb(t)::text FROM device_routes t
 UNION ALL SELECT to_jsonb(t)::text FROM memberships t
) fixture;
SQL
BASELINE=$(sql < "$TMP/fingerprint.sql")
printf '\nFASE 2: upgrade N+1, 0008 y escrituras nuevas\n'
migrate "$NEW_DIGEST"
check_versions 1,2,3,4,5,6,7,8
ok 'historial 1–8 exitoso'
[ "$HISTORY" = "$(sql -c "SELECT string_agg(version::text||':'||encode(checksum,'hex'),',' ORDER BY version) FROM _sqlx_migrations WHERE version<=7")" ]
ok 'checksums 1–7 intactos'
[ "$BASELINE" = "$(sql < "$TMP/fingerprint.sql")" ]; ok 'valores de toda la siembra intactos al migrar'
expect_sql "SELECT count(*) FROM information_schema.columns WHERE table_name IN ('devices','enrollment_tokens') AND column_name='role' AND column_default=quote_literal('device')||'::text' AND is_nullable='NO'" 2 'ambas columnas role NOT NULL con default device'
expect_sql "SELECT count(*) FROM pg_constraint WHERE conrelid IN ('devices'::regclass,'enrollment_tokens'::regclass) AND contype='c' AND pg_get_constraintdef(oid) LIKE '%role%'" 2 'ambos CHECK de role presentes'
expect_sql "SELECT count(*) FROM devices WHERE role<>'device'" 0 'dispositivos de N reciben device'
expect_sql "SELECT count(*) FROM enrollment_tokens WHERE role<>'device'" 0 'tokens de N reciben device'
start "$NEW_DIGEST"
probe nuevo
stop
printf '\nFASE 3: retroceso por ID, arranque normal N sin --migrate\n'
start "$OLD_DIGEST"
probe retroceso
stop
expect_sql "SELECT count(*) FROM devices WHERE role='server'" 2 'N conserva las dos filas server al aprobar, emitir y revocar'
expect_sql "SELECT count(*) FROM devices WHERE name='lab-rollback' AND role='device' AND status='approved'" 1 'INSERT de N toma default device'
expect_sql "SELECT count(*) FROM audit_events WHERE action='member.set' AND detail->>'previous_role'='member' AND detail->>'role'='admin'" 1 'member.set conserva previous_role y role'
# Sin procesos activos: compara todas las tablas, secuencias e historial antes/después.
printf '\nFASE 4: negativo N --migrate sobre 0008\n'
dump_hash() {
  docker exec "$PG" pg_dump -U postgres -d atenea --no-owner --no-privileges |
    docker run --rm -i --network none "$PROBE_IMAGE" python3 -c \
      'import hashlib,sys; print(hashlib.sha256(b"".join(x for x in sys.stdin.buffer if not x.startswith((b"\\restrict ",b"\\unrestrict ")))).hexdigest())'
}
BEFORE=$(dump_hash)
set +e
migrate "$OLD_DIGEST" > "$TMP/negative.txt" 2>&1
NEGATIVE_RC=$?
set -e
printf 'NEGATIVO exit=%s; salida exacta:\n' "$NEGATIVE_RC"
cat "$TMP/negative.txt"
[ "$NEGATIVE_RC" -ne 0 ]; ok 'N --migrate rechaza BD con 0008 (el binario oculta la causa interna)'
[ "$BEFORE" = "$(dump_hash)" ]; ok 'base completa e historial no cambiaron tras el negativo'
printf '\nFASE 5: re-upgrade N+1 idempotente y persistencia\n'
BEFORE=$(dump_hash)
migrate "$NEW_DIGEST"
[ "$BEFORE" = "$(dump_hash)" ]; ok 'migración N+1 idempotente, sin cambios en la base'
check_versions 1,2,3,4,5,6,7,8
ok 'historial final 1–8 exitoso'
start "$NEW_DIGEST"
probe reupgrade
stop
expect_sql "SELECT count(*) FROM enrollment_tokens WHERE role='server' AND consumed_at IS NOT NULL" 1 'token server consumido y conservado'
expect_sql "SELECT count(*) FROM enrollment_tokens t JOIN devices d ON d.id=t.consumed_by_device WHERE d.name='lab-rollback' AND t.role='device'" 1 'token creado por N conserva default device'
expect_sql "SELECT count(*) FROM enrollment_tokens WHERE consumed_at IS NULL AND role='device'" 1 'token sin usar de N conservado'
printf 'PASS retroceso 0008 completo en %s s; solo control plane/PostgreSQL sintéticos\n' "$((SECONDS-START))"
