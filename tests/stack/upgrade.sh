#!/usr/bin/env bash
# Upgrade real N (commit anterior a 0003) -> N+1 y rollback por ID sha256 local.
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
OLD_IMAGE=atenea/control-plane:upgrade-n-$$
NEW_IMAGE=atenea/control-plane:upgrade-next-$$
POSTGRES_IMAGE=postgres:17@sha256:b8faeff7712663c2773a190181e1246e37e364b887611c04658c28b44e2408fe
PROBE_IMAGE=${UPGRADE_PROBE_IMAGE:-atenea-panel-lab:latest}
cleanup() {
  local rc=$?
  trap - EXIT
  if [ "$rc" -ne 0 ]; then
    docker logs "$CP" 2>&1 || true
    docker logs "$PG" 2>&1 || true
  fi
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
INTRODUCED=$(git log -1 --diff-filter=A --format=%H -- apps/control-plane/migrations/0003_routes_dns.sql)
test -n "$INTRODUCED"
PREVIOUS=$(git rev-parse "$INTRODUCED^")
git worktree add --detach "$TMP/previous" "$PREVIOUS"
set -- "$TMP/previous"/apps/control-plane/migrations/*.sql
[ "$#" -eq 2 ]
[ "$(basename "$1")" = 0001_control_plane.sql ]
[ "$(basename "$2")" = 0002_panel.sql ]
test -f apps/control-plane/migrations/0003_routes_dns.sql
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
  -e ATENEA_CP_PUBLIC_URL=http://control-plane:8080)
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
migrate "$OLD_DIGEST"
check_versions 1,2
# Reutilizar el vector firmado del repositorio, no bytes que el API no pueda parsear.
bundle_hex=$(od -An -v -tx1 tests/fixtures/identity/valid_bundle.bin | tr -d ' \n')
sql -v bundle="$bundle_hex" -v app_password="$APP_PASSWORD" -v session="$SESSION_TOKEN" \
  -v csrf="$CSRF_TOKEN" -v enrollment="$ENROLLMENT_TOKEN" <<'SQL'
ALTER ROLE atenea_app PASSWORD :'app_password';
INSERT INTO users(id,oidc_issuer,oidc_subject,email,display_name) VALUES ('00000000-0000-0000-0000-000000000001','https://upgrade.invalid','upgrade-owner','upgrade@example.invalid','Upgrade owner');
INSERT INTO organizations(id,name) VALUES ('00000000-0000-0000-0000-000000000002','upgrade-fixture');
INSERT INTO memberships VALUES ('00000000-0000-0000-0000-000000000002','00000000-0000-0000-0000-000000000001','owner');
INSERT INTO sessions(id,user_id,csrf_hash,csrf_token,expires_at) VALUES (sha256(:'session'::bytea),'00000000-0000-0000-0000-000000000001',sha256(:'csrf'::bytea),:'csrf',now()+interval '8 hours');
INSERT INTO networks(id,org_id,name,ipv4_cidr,ipv6_cidr,root_anchor) VALUES ('00000000-0000-0000-0000-000000000003','00000000-0000-0000-0000-000000000002','upgrade-net','10.42.0.0/24','fd42::/64',decode(repeat('01',32),'hex'));
INSERT INTO devices(id,network_id,name,status,noise_pubkey,identity_pubkey,bundle,approved_at) VALUES ('00000000-0000-0000-0000-000000000004','00000000-0000-0000-0000-000000000003','upgrade-device','approved',decode(repeat('02',32),'hex'),decode(repeat('03',32),'hex'),decode(:'bundle','hex'),now());
INSERT INTO ip_allocations VALUES ('00000000-0000-0000-0000-000000000003','00000000-0000-0000-0000-000000000004',4,'10.42.0.2'),('00000000-0000-0000-0000-000000000003','00000000-0000-0000-0000-000000000004',6,'fd42::2');
INSERT INTO enrollment_tokens(id,network_id,token_hash,created_by,expires_at) VALUES ('00000000-0000-0000-0000-000000000005','00000000-0000-0000-0000-000000000003',sha256(:'enrollment'::bytea),'00000000-0000-0000-0000-000000000001',now()+interval '1 day');
INSERT INTO access_policies(id,network_id,src,dst,proto,port_min,port_max,created_by) VALUES ('00000000-0000-0000-0000-000000000006','00000000-0000-0000-0000-000000000003','{"type":"any"}','{"type":"any"}',6,443,443,'00000000-0000-0000-0000-000000000001');
INSERT INTO configuration_versions(device_id,version,config_hash,not_before,not_after,issuer_serial) VALUES ('00000000-0000-0000-0000-000000000004',7,sha256('upgrade-config'::bytea),now(),now()+interval '1 hour',1);
INSERT INTO audit_events(actor_kind,actor_id,org_id,network_id,action,target,detail) VALUES ('system','upgrade','00000000-0000-0000-0000-000000000002','00000000-0000-0000-0000-000000000003','upgrade.fixture','fixture','{"persistent":true}');
INSERT INTO device_reports(device_id,reported_at,nat_mapping,paths) VALUES ('00000000-0000-0000-0000-000000000004',now(),'independent','[]');
INSERT INTO network_samples VALUES ('00000000-0000-0000-0000-000000000003',now()-interval '1 minute',1,0,1,0,0,0);
SQL
# Solo columnas de N: 0003 añade campos, y sesiones/muestreador actualizan timestamps.
cat > "$TMP/fingerprint.sql" <<'SQL'
SELECT md5(string_agg(v,E'\n' ORDER BY v)) FROM (
 SELECT row_to_json(t)::text v FROM users t
 UNION ALL SELECT row_to_json(t)::text FROM organizations t WHERE name='upgrade-fixture'
 UNION ALL SELECT row_to_json(t)::text FROM memberships t WHERE org_id='00000000-0000-0000-0000-000000000002'
 UNION ALL SELECT json_build_array(id,user_id,csrf_hash,csrf_token,expires_at)::text FROM sessions
 UNION ALL SELECT json_build_array(id,org_id,name,ipv4_cidr,ipv6_cidr,root_anchor,issuer_cert,created_at)::text FROM networks
 UNION ALL SELECT json_build_array(id,network_id,name,status,noise_pubkey,identity_pubkey,bundle,endpoints,enrolled_at,approved_at,revoked_at,last_config_at)::text FROM devices
 UNION ALL SELECT row_to_json(t)::text FROM ip_allocations t
 UNION ALL SELECT row_to_json(t)::text FROM enrollment_tokens t
 UNION ALL SELECT row_to_json(t)::text FROM access_policies t
 UNION ALL SELECT row_to_json(t)::text FROM configuration_versions t
 UNION ALL SELECT row_to_json(t)::text FROM audit_events t WHERE action='upgrade.fixture'
 UNION ALL SELECT json_build_array(device_id,reported_at,nat_mapping,paths)::text FROM device_reports
 UNION ALL SELECT row_to_json(t)::text FROM network_samples t WHERE relay=1
) fixture;
SQL
BASELINE=$(sql < "$TMP/fingerprint.sql")
check_data() { [ "$(sql < "$TMP/fingerprint.sql")" = "$BASELINE" ]; }
probe() {
  docker run --rm -i --network "$NET" -e "UPGRADE_SESSION=$SESSION_TOKEN" \
    -e "UPGRADE_CSRF=$CSRF_TOKEN" "$PROBE_IMAGE" python3 - "$1" <<'PY'
import json, os, sys, time, urllib.request, urllib.error
stage = sys.argv[1]
base = 'http://control-plane:8080'
def request(path, body=None, method=None):
    headers = {'Cookie':'atenea_session='+os.environ['UPGRADE_SESSION'], 'X-CSRF-Token':os.environ['UPGRADE_CSRF']}
    if body is not None:
        headers['Content-Type'] = 'application/json'
    req = urllib.request.Request(base+path, data=None if body is None else json.dumps(body).encode(), headers=headers, method=method)
    try:
        with urllib.request.urlopen(req, timeout=5) as res:
            data = res.read()
            return res.status, json.loads(data) if data else None
    except urllib.error.HTTPError as error:
        raise RuntimeError(f'{stage} {req.get_method()} {path}: HTTP {error.code} {error.read().decode()}') from error
for attempt in range(60):
    try:
        assert request('/healthz')[0] == 200
        break
    except Exception:
        if attempt == 59: raise
        time.sleep(1)
net = '/api/v1/networks/00000000-0000-0000-0000-000000000003'
assert request(net)[1]['name'] == 'upgrade-net'
assert request(net+'/devices')[1][0]['name'] == 'upgrade-device'
assert len(request(net+'/policies')[1]) == 1
assert request('/api/v1/session')[1]['user']['email'] == 'upgrade@example.invalid'
status, org = request('/api/v1/orgs', {'name':'write-'+stage})
assert status == 201 and org['name'] == 'write-'+stage, org
assert any(o['id'] == org['id'] for o in request('/api/v1/orgs')[1])
if stage == 'next':
    dns = {'servers':['10.42.0.2'],'domains':['upgrade.internal']}
    assert request(net+'/dns', dns, 'PUT')[0] == 204
    assert request(net+'/dns')[1] == dns
print('PASS HTTP '+stage+': sesión, red, dispositivo, política y escritura persistente'+(' + DNS 0003' if stage == 'next' else ''))
PY
}
start "$OLD_DIGEST"
probe initial
check_data
printf 'PASS N schema=1,2; datos=%s\n' "$BASELINE"
stop
migrate "$NEW_DIGEST"
check_versions 1,2,3,4
start "$NEW_DIGEST"
probe next
check_data
stop
# Compatibilidad explícita de N antes de realizar el rollback definitivo.
start "$OLD_DIGEST"
probe compatibility
check_data
check_versions 1,2,3,4
printf 'PASS N sobre schema N+1 (sin down migrations)\n'
stop
start "$NEW_DIGEST"
probe before-rollback
stop
# No se resuelve un tag mutable para rollback: se usa el ID sha256 capturado antes.
start "$OLD_DIGEST"
probe rollback
check_data
check_versions 1,2,3,4
[ "$(sql -c "SELECT count(*) FROM organizations WHERE name IN ('write-initial','write-next','write-compatibility','write-before-rollback','write-rollback')")" = 5 ]
[ "$(sql -c "SELECT dns_servers::text || '|' || dns_domains::text FROM networks WHERE id='00000000-0000-0000-0000-000000000003'")" = '["10.42.0.2"]|["upgrade.internal"]' ]
printf 'PASS rollback digest=%s; datos=%s; 5 escrituras y DNS N+1 conservados\n' "$OLD_DIGEST" "$BASELINE"
stop
printf 'PASS upgrade/rollback PostgreSQL real; contenedores y worktree efímeros\n'
printf 'PASS upgrade completo en %s s (incluye ambas compilaciones offline)\n' "$((SECONDS-START))"
