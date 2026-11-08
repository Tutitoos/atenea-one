#!/usr/bin/env bash
# Copyright 2026 Tutitoos
# SPDX-License-Identifier: Apache-2.0
# Retroceso v0.1.16 (binario desplegado en el piloto) sobre 0010 (devices.services/services_at) con escrituras nuevas.
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
PREVIOUS=$(git rev-parse v0.1.16^{commit})
git worktree add --detach "$TMP/previous" "$PREVIOUS"
set -- "$TMP/previous"/apps/control-plane/migrations/*.sql
[ "$#" -eq 9 ]
test -f "$TMP/previous/apps/control-plane/migrations/0009_service_tokens.sql"
set -- apps/control-plane/migrations/*.sql
[ "$#" -eq 10 ]
test -f apps/control-plane/migrations/0010_device_services.sql
printf 'N tag v0.1.16 commit=%s; N+1 working-tree HEAD=%s\n' "$PREVIOUS" "$(git rev-parse HEAD)"

# Compilación con las cachés de laboratorio existentes; rustup no interviene.
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
# La caché de registro del laboratorio puede no tener las versiones de Cargo.lock (utoipa 6): cargo usa red solo para crates.io.
build() {
  local source=$1 image=$2
  docker run --rm -e RUSTUP_TOOLCHAIN=1.99.1 -e CARGO_BUILD_JOBS=2 -e CARGO_TARGET_DIR=/cache/target \
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
      cargo build --release --locked -p atenea-control-plane
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
checksums() { sql -c "SELECT string_agg(version::text||':'||encode(checksum,'hex'),',' ORDER BY version) FROM _sqlx_migrations WHERE version<=$1"; }
# Columnas nuevas por dispositivo (nombre estable; excluye el dispositivo que crea N en el retroceso).
services_state() { sql -c "SELECT string_agg(name||':'||services::text||':'||coalesce(services_at::text,'null'),',' ORDER BY name) FROM devices WHERE name<>'lab-rollback'"; }
printf '\nFASE 1: N v0.1.16, migraciones 1–9 y siembra\n'
migrate "$OLD_DIGEST"
check_versions 1,2,3,4,5,6,7,8,9
ok 'N: historial 1–9 exitoso'
HISTORY=$(checksums 9)
sql -v app_password="$APP_PASSWORD" -v session="$SESSION_TOKEN" -v csrf="$CSRF_TOKEN" <<'SQL'
ALTER ROLE atenea_app PASSWORD :'app_password';
INSERT INTO users(id,oidc_issuer,oidc_subject,email,display_name) VALUES
('00000000-0000-0000-0000-000000000001','https://rollback.invalid','owner','owner@example.invalid','Propietario sintético');
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
        # Solo el código de error de la API (cuerpo JSON corto), nunca material de sesión.
        raise RuntimeError(f'{stage} {method} {path}: HTTP {e.code} {e.read()[:60]!r}') from None
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
if stage=='N':
    s['org']=req('/api/v1/orgs',{'name':'rollback-org'})['id']
    root=cli('root-init','--out',str(p/'root.key')).splitlines()[-1]
    s['root']=root
    s['net']=req('/api/v1/orgs/'+s['org']+'/networks',{'name':'rollback-net','ipv4_cidr':'10.42.0.0/24','ipv6_cidr':'fd42::/64','root_anchor':root})['id']
net='/api/v1/networks/'+s.get('net','')
org='/api/v1/orgs/'+s.get('org','')
if stage=='N':
    cli('issuer-cert','--root',str(p/'root.key'),'--issuer-key',str(p/'issuer.key'),'--network-id',s['net'].replace('-',''),'--serial','1','--valid-days','30','--out',str(p/'issuer.cert'))
    req(net+'/issuer-cert',(p/'issuer.cert').read_bytes(),'PUT',raw=True)
    enroll('approved',approve=True)
    enroll('peer',approve=True)
    enroll('pending')
    s['policy']=req(net+'/policies',policy)['id']
    ck(any(d['id']==s['pending'] and d['status']=='pending' for d in req(net+'/devices')),'siembra: dos aprobados, uno pendiente y política')
elif stage=='nuevo':
    devices=req(net+'/devices')
    ck(len(devices)==3 and all(d['services']==[] and d['services_at'] is None for d in devices),'N+1: dispositivos migrados con services [] y services_at nulo')
    ck(all(d['services']==[] for d in req('/api/v1/device/peers',device='approved')),'N+1: peers sin servicios antes de informar')
    req('/api/v1/device/status',{'paths':[],'services':svc},device='peer')
    ck(any(d['id']==s['peer'] and d['services']==[dict(svc[0])] and d['services_at'] for d in req(net+'/devices')),'N+1: informe firmado guarda servicios y services_at')
    peers=req('/api/v1/device/peers',device='approved')
    ck(any(d['device_id']==s['peer'] and d['services']==svc for d in peers),'N+1: /device/peers de un par con política muestra los servicios')
    ck(not any(d['device_id']==s['pending'] for d in peers),'N+1: pendiente no aparece en peers')
elif stage=='retroceso':
    ck(any(o['id']==s['org'] for o in req('/api/v1/orgs')),'organización legible')
    ck(any(n['id']==s['net'] for n in req(org+'/networks')),'lista de redes legible')
    ck(req(net)['name']=='rollback-net','detalle de red legible')
    ck(len(req(net+'/devices'))==3,'dispositivos legibles')
    ck(any(d['id']==s['policy'] for d in req(net+'/policies')),'política conservada')
    ck(isinstance(req(net+'/audit'),list),'auditoría legible')
    ck(any(d['device_id']==s['peer'] for d in req('/api/v1/device/peers',device='approved')),'N: /device/peers firmado lista al par')
    ck(len(req('/api/v1/device/config',device='approved',raw=True))>100,'N emite configuración firmada')
    # Informe de N sin services: no debe tocar las columnas nuevas (comprobado por SQL fuera del cliente).
    ck(req('/api/v1/device/status',{'paths':[]},device='approved') is None,'N acepta informe firmado de dispositivo aprobado (204)')
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
    ck(any(d['id']==s['peer'] and d['services']==[dict(svc[0])] and d['services_at'] for d in devices),'N+1 vuelve a leer los servicios escritos antes del retroceso')
    ck(all(d['services']==[] and d['services_at'] is None for d in devices if d['id']!=s['peer']),'resto de dispositivos sigue con services [] y services_at nulo')
    ck(any(o['id']==s['rollback_org'] for o in req('/api/v1/orgs')),'organización escrita por N conservada')
    ck(any(n['id']==s['rollback_net'] for n in req('/api/v1/orgs/'+s['rollback_org']+'/networks')),'red escrita por N conservada')
    # Los servicios caducan a los 120 s en peers; un informe nuevo con N+1 los vuelve a publicar.
    # El control plane limita los informes a uno cada 5 s por reloj de la base; el laboratorio puede ir más rápido que eso: se reintenta.
    for attempt in range(60):
        try:
            req('/api/v1/device/status',{'paths':[],'services':svc},device='peer'); break
        except RuntimeError as e:
            if 'rate_limited' not in str(e) or attempt==59: raise
            time.sleep(1)
    ck(any(d['device_id']==s['peer'] and d['services']==svc for d in req('/api/v1/device/peers',device='approved')),'N+1: peers vuelve a mostrar servicios tras nuevo informe')
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
cat > "$TMP/fingerprint.sql" <<'SQL'
SELECT md5(string_agg(v,E'\n' ORDER BY v)) FROM (
 SELECT jsonb_strip_nulls(to_jsonb(t)-'services'-'services_at')::text v FROM devices t
 UNION ALL SELECT to_jsonb(t)::text FROM enrollment_tokens t
 UNION ALL SELECT to_jsonb(t)::text FROM users t
 UNION ALL SELECT to_jsonb(t)::text FROM organizations t
 UNION ALL SELECT to_jsonb(t)::text FROM networks t
 UNION ALL SELECT to_jsonb(t)::text FROM access_policies t
 UNION ALL SELECT to_jsonb(t)::text FROM memberships t
) fixture;
SQL
BASELINE=$(sql < "$TMP/fingerprint.sql")
printf '\nFASE 2: upgrade N+1, 0010 y escritura de servicios\n'
migrate "$NEW_DIGEST"
check_versions 1,2,3,4,5,6,7,8,9,10
ok 'historial 1–10 exitoso'
[ "$HISTORY" = "$(checksums 9)" ]
ok 'checksums 1–9 intactos'
HISTORY10=$(checksums 10)
[ "$BASELINE" = "$(sql < "$TMP/fingerprint.sql")" ]; ok 'valores de toda la siembra intactos al migrar'
expect_sql "SELECT column_default||'|'||is_nullable||'|'||data_type FROM information_schema.columns WHERE table_name='devices' AND column_name='services'" "'[]'::jsonb|NO|jsonb" 'devices.services jsonb NOT NULL DEFAULT []'
expect_sql "SELECT is_nullable||'|'||data_type FROM information_schema.columns WHERE table_name='devices' AND column_name='services_at'" 'YES|timestamp with time zone' 'devices.services_at timestamptz nulo permitido'
expect_sql "SELECT count(*) FROM devices WHERE services<>'[]'::jsonb OR services_at IS NOT NULL" 0 'filas previas con services [] y services_at NULL'
start "$NEW_DIGEST"
probe nuevo
stop
SERVICES_N1=$(services_state)
expect_sql "SELECT count(*) FROM devices WHERE name='lab-peer' AND jsonb_array_length(services)=1 AND services_at IS NOT NULL" 1 'servicios escritos por N+1 persistidos en la base'
printf '\nFASE 3: retroceso por ID, arranque normal N sin --migrate\n'
start "$OLD_DIGEST"
probe retroceso
stop
[ "$SERVICES_N1" = "$(services_state)" ]; ok 'N no tocó services/services_at de los dispositivos existentes'
expect_sql "SELECT services::text||'|'||coalesce(services_at::text,'null') FROM devices WHERE name='lab-rollback'" "[]|null" 'dispositivo creado por N toma services [] y services_at NULL por defecto'
check_versions 1,2,3,4,5,6,7,8,9,10
ok 'historial 1–10 intacto tras arrancar N'
# Sin procesos activos: compara todas las tablas, secuencias e historial antes/después.
printf '\nFASE 4: negativo N --migrate sobre 0010\n'
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
[ "$NEGATIVE_RC" -ne 0 ]; ok 'N --migrate rechaza BD con 0010 (código distinto de 0)'
[ "$BEFORE" = "$(dump_hash)" ]; ok 'base completa e historial no cambiaron tras el negativo'
printf '\nFASE 5: re-upgrade N+1 idempotente y persistencia\n'
BEFORE=$(dump_hash)
migrate "$NEW_DIGEST"
[ "$BEFORE" = "$(dump_hash)" ]; ok 'migración N+1 idempotente, sin cambios en la base'
check_versions 1,2,3,4,5,6,7,8,9,10
ok 'historial final 1–10 exitoso'
[ "$HISTORY10" = "$(checksums 10)" ]; ok 'checksums 1–10 iguales tras la nueva subida'
start "$NEW_DIGEST"
probe reupgrade
stop
printf 'PASS retroceso 0010 completo en %s s; solo control plane/PostgreSQL sintéticos\n' "$((SECONDS-START))"
