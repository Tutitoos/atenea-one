#!/usr/bin/env bash
# E2E de la fase 3B dentro del contenedor del laboratorio (ver run-e2e.sh).
# Control plane real (proceso) + PostgreSQL + Dex + dos agentes que se enrolan, sincronizan y se comunican.
set -uo pipefail

LAB=/tmp/lab
TOOLS=$PWD/tests/network-lab/tools.py
CP=$PWD/tests/network-lab/cp.py
export CP_URL=http://192.0.2.254:8080 CP_JAR=$LAB/cp.cookies
FAILS=0
ok()   { echo "  OK   $*"; }
bad()  { echo "  FAIL $*"; FAILS=$((FAILS + 1)); }
check() { local d=$1; shift; if "$@" >/dev/null 2>&1; then ok "$d"; else bad "$d"; fi; }
expect_eq() { if [ -n "$2" ] && [ "$2" = "$3" ]; then ok "$1 ($2)"; else bad "$1: esperado '$3', obtenido '$2'"; fi; }
api()   { python3 "$CP" api "$@"; }
field() { python3 "$TOOLS" field "$LAB/$1.sock" "$2"; }
ctr()   { python3 "$TOOLS" counter "$LAB/$1.sock" "$2"; }
section() { echo; echo "== $*"; }
uuid_of() { python3 -c "import uuid,sys;print(uuid.UUID(sys.argv[1]))" "$1"; }

section "Compilación"
cargo build --release --locked -q 2>&1 | tail -3
BIN=$CARGO_TARGET_DIR/release
export PATH=$BIN:$PATH
rm -rf "$LAB"; mkdir -p "$LAB"

section "Infraestructura: PostgreSQL, Dex y control plane"
ADMIN_URL=postgres://postgres:pg@postgres/atenea
for _ in $(seq 1 60); do psql "$ADMIN_URL" -qc 'select 1' >/dev/null 2>&1 && break; sleep 1; done
check "PostgreSQL responde" psql "$ADMIN_URL" -qc 'select 1'
for _ in $(seq 1 60); do python3 -c "import urllib.request;urllib.request.urlopen('http://dex:5556/dex/.well-known/openid-configuration',timeout=2)" 2>/dev/null && break; sleep 1; done
check "Dex responde" python3 -c "import urllib.request;urllib.request.urlopen('http://dex:5556/dex/.well-known/openid-configuration',timeout=2)"

ip link add br0 type bridge && ip link set br0 up && ip addr add 192.0.2.254/24 dev br0
i=0
for n in a b; do
  i=$((i + 1))
  ip netns add $n
  ip link add va-$n type veth peer name v$n
  ip link set v$n netns $n
  ip link set va-$n master br0 up
  ip -n $n link set lo up
  ip -n $n addr add 192.0.2.$i/24 dev v$n
  ip -n $n link set v$n up
done

psql "$ADMIN_URL" -qc "CREATE ROLE atenea_app LOGIN PASSWORD 'app-pass'" >/dev/null
python3 -c "import secrets;print(secrets.token_hex(32))" > $LAB/issuer.key; chmod 600 $LAB/issuer.key
export ATENEA_CP_PUBLIC_URL=$CP_URL
export ATENEA_CP_OIDC_ISSUER=http://dex:5556/dex
export ATENEA_CP_OIDC_CLIENT_ID=atenea-control-plane
export ATENEA_CP_OIDC_CLIENT_SECRET=dev-only-secret-not-for-production
export ATENEA_CP_OIDC_REDIRECT_URL=$CP_URL/auth/callback
export ATENEA_CP_ISSUER_KEY_FILE=$LAB/issuer.key
export ATENEA_CP_CONFIG_VALIDITY_SECS=40
export ATENEA_CP_DEV_INSECURE_COOKIES=true
export ATENEA_CP_LISTEN=0.0.0.0:8080
ATENEA_CP_DATABASE_URL=$ADMIN_URL ATENEA_CP_APPLICATION_ROLE=atenea_app atenea-control-plane --migrate > $LAB/migrate.log 2>&1
check "migraciones aplicadas con el rol de aplicación limitado" test $? -eq 0
export ATENEA_CP_DATABASE_URL=postgres://atenea_app:app-pass@postgres/atenea
start_cp() { atenea-control-plane > $LAB/cp.log 2>&1 & CPPID=$!; }
start_cp
for _ in $(seq 1 30); do python3 -c "import urllib.request;urllib.request.urlopen('$CP_URL/healthz',timeout=2)" 2>/dev/null && break; sleep 1; done
check "control plane en /healthz" python3 -c "import urllib.request;urllib.request.urlopen('$CP_URL/healthz',timeout=2)"

section "Administración por OIDC real (Dex): organización, red y certificado de emisor"
python3 "$CP" login admin@example.test password > $LAB/login.txt 2>&1
expect_eq "inicio de sesión OIDC (Code + PKCE) completado" "$(cat $LAB/login.txt)" admin@example.test
[ "$(cat $LAB/login.txt)" = admin@example.test ] || { cat $LAB/cp.log | tail -20; exit 1; }
ORG=$(api POST /api/v1/orgs --json '{"name":"lab"}' --field id)
atenea root-init --out $LAB/root.key > $LAB/root.pub
ROOT_PUB=$(cat $LAB/root.pub)
NETID=$(api POST /api/v1/orgs/$ORG/networks --json "{\"name\":\"lab-net\",\"ipv4_cidr\":\"10.77.0.0/24\",\"ipv6_cidr\":\"fd77::/64\",\"root_anchor\":\"$ROOT_PUB\"}" --field id)
[ -n "$NETID" ] && ok "organización y red creadas ($NETID)" || bad "no se creó la red"
NETHEX=${NETID//-/}
atenea issuer-cert --root $LAB/root.key --issuer-key $LAB/issuer.key --network-id $NETHEX --serial 1 --valid-days 30 --out $LAB/issuer.cert
check "certificado de emisor subido y validado por el control plane" api PUT /api/v1/networks/$NETID/issuer-cert --file $LAB/issuer.cert
expect_eq "la red declara tener certificado de emisor" "$(api GET /api/v1/networks/$NETID --field has_issuer_cert)" true

section "Enrolamiento, aprobación y políticas"
enroll() {  # enroll <ns>: crea claves, token, enrola; imprime el device_id (UUID)
  local n=$1
  atenea device-init --dir $LAB/$n > $LAB/$n.info
  api POST /api/v1/networks/$NETID/enrollment-tokens --json '{"ttl_secs":600}' > $LAB/$n.token.json
  python3 -c "import json;print(json.dumps(json.load(open('$LAB/$n.token.json'))['join']))" > $LAB/$n.join.json
  atenea enroll --url $CP_URL --token "$(python3 -c "import json;print(json.load(open('$LAB/$n.token.json'))['token'])")" --dir $LAB/$n --name host-$n > $LAB/$n.enroll.txt 2>&1
}
enroll a; enroll b
DEV_A=$(uuid_of "$(sed -n 's/^device_id=//p' $LAB/a.info)")
DEV_B=$(uuid_of "$(sed -n 's/^device_id=//p' $LAB/b.info)")
grep -q pending $LAB/a.enroll.txt && ok "A enrolado en estado pending" || { bad "A no queda pending"; cat $LAB/a.enroll.txt; }
TOKEN_A=$(python3 -c "import json;print(json.load(open('$LAB/a.token.json'))['token'])")
check "un token de un solo uso no se puede reutilizar" bash -c "! atenea enroll --url $CP_URL --token '$TOKEN_A' --dir $LAB/a --name otra"
FP_A=$(sed -n 's/^fingerprint=//p' $LAB/a.info)
check "el inventario muestra a A con su huella ($FP_A)" bash -c "python3 $CP api GET /api/v1/networks/$NETID/devices | grep -q '$FP_A'"
for D in $DEV_A $DEV_B; do api POST /api/v1/networks/$NETID/devices/$D/approve > /dev/null && ok "dispositivo $D aprobado (IPv4+IPv6 asignadas)" || bad "no se aprobó $D"; done
api PATCH /api/v1/networks/$NETID/devices/$DEV_A --json '{"endpoints":["192.0.2.1:51820"]}' >/dev/null
api PATCH /api/v1/networks/$NETID/devices/$DEV_B --json '{"endpoints":["192.0.2.2:51820"]}' >/dev/null
api POST /api/v1/networks/$NETID/policies --json '{"src":{"kind":"any"},"dst":{"kind":"any"},"proto":1}' >/dev/null
api POST /api/v1/networks/$NETID/policies --json '{"src":{"kind":"any"},"dst":{"kind":"any"},"proto":58}' >/dev/null
api POST /api/v1/networks/$NETID/policies --json "{\"src\":{\"kind\":\"device\",\"device_id\":\"$DEV_A\"},\"dst\":{\"kind\":\"device\",\"device_id\":\"$DEV_B\"},\"proto\":6,\"port_min\":9000,\"port_max\":9100}" >/dev/null && ok "políticas: ICMP libre y TCP A→B 9000-9100"

section "Agentes: configuración generada, sincronización y túnel"
for n in a b; do
  atenea agent-config --join "$(cat $LAB/$n.join.json)" --dir $LAB/$n --out $LAB/$n.toml --state-dir $LAB/$n.state --config-file $LAB/$n.cfg --ipc-socket $LAB/$n.sock
  echo "interval_secs = 10" >> $LAB/$n.toml
done
ip netns exec a atenea up --config $LAB/a.toml --log $LAB/a.log >/dev/null && ip netns exec b atenea up --config $LAB/b.toml --log $LAB/b.log >/dev/null && ok "agentes arrancados" || { bad "arranque"; cat $LAB/a.log $LAB/b.log; exit 1; }
for _ in $(seq 1 30); do [ "$(field a config.lease_active)" = true ] && [ "$(field b config.lease_active)" = true ] && break; sleep 1; done
expect_eq "A obtuvo una configuración vigente del control plane" "$(field a config.lease_active)" true
expect_eq "B obtuvo una configuración vigente del control plane" "$(field b config.lease_active)" true
IP_A4=$(ip -n a -4 -o addr show dev atenea0 | python3 -c "import sys,re;print(re.search(r'inet (\S+?)/',sys.stdin.read()).group(1))")
IP_B4=$(ip -n b -4 -o addr show dev atenea0 | python3 -c "import sys,re;print(re.search(r'inet (\S+?)/',sys.stdin.read()).group(1))")
IP_B6=$(ip -n b -6 -o addr show dev atenea0 scope global | python3 -c "import sys,re;print(re.search(r'inet6 (\S+?)/',sys.stdin.read()).group(1))")
echo "       direcciones asignadas por IPAM: A=$IP_A4  B=$IP_B4 / $IP_B6"
check "ping IPv4 A→B" ip netns exec a ping -c 3 -W 3 $IP_B4
check "ping IPv6 A→B" ip netns exec a ping -6 -c 3 -W 3 $IP_B6
ip netns exec b python3 "$TOOLS" tcp-server $IP_B4 9000 > $LAB/srv.json &
S=$!
ip netns exec a python3 "$TOOLS" tcp-client $IP_B4 9000 2000000 5 > $LAB/cli.json
wait $S
expect_eq "TCP 2 MB A→B íntegro (política permite 9000-9100)" "$(python3 -c "import json;print(json.load(open('$LAB/srv.json'))['sha256'])")" "$(python3 -c "import json;print(json.load(open('$LAB/cli.json'))['sha256'])")"
D0=$(ctr a tx_acl_denied)
ip netns exec a python3 "$TOOLS" send-udp $IP_B4 9500 "fuera de política"
sleep 1
expect_eq "UDP sin política descartado en origen" "$(( $(ctr a tx_acl_denied) - D0 ))" 1
check "el control plane registró la auditoría de la red" bash -c "python3 $CP api GET '/api/v1/networks/$NETID/audit?limit=100' | python3 -c \"import sys;t=sys.stdin.read();assert all(a in t for a in ['device.enroll','device.approve','policy.create','config.issue'])\""

section "Revocación (ADR-002: validez 40 s, tolerancia 30 s, sondeo del agente cada 10 s)"
T_REV=$(date +%s)
api POST /api/v1/networks/$NETID/devices/$DEV_A/revoke >/dev/null && ok "A revocado en el control plane" || bad "revocación"
FAILS_IN_ROW=0
while [ $(( $(date +%s) - T_REV )) -lt 40 ]; do
  if ip netns exec a ping -c 1 -W 1 $IP_B4 >/dev/null 2>&1; then FAILS_IN_ROW=0; else FAILS_IN_ROW=$((FAILS_IN_ROW + 1)); fi
  [ $FAILS_IN_ROW -ge 2 ] && break
  sleep 0.5
done
CUT_PEER=$(( $(date +%s) - T_REV ))
[ $FAILS_IN_ROW -ge 2 ] && [ $CUT_PEER -le 20 ] && ok "B deja de aceptar a A en ${CUT_PEER}s (sondeo de B ≤ 10 s + recarga)" || bad "B no cortó a A a tiempo (${CUT_PEER}s)"
for _ in $(seq 1 15); do [ "$(field a sync.sync_denied)" -ge 1 ] && break; sleep 1; done
[ "$(field a sync.sync_denied)" -ge 1 ] && ok "A recibe 403 del control plane (no se le emite más configuración)" || bad "A no registra sync_denied"
for _ in $(seq 1 100); do [ "$(field a config.lease_active)" = false ] && break; sleep 1; done
CUT_SELF=$(( $(date +%s) - T_REV ))
[ "$(field a config.lease_active)" = false ] && [ $CUT_SELF -le 72 ] && ok "el propio A se corta al caducar su última configuración en ${CUT_SELF}s (límite 40+30 s tras la última emisión)" || bad "A no se cortó a sí mismo a tiempo (${CUT_SELF}s)"

section "Pérdida del canal de control: el agente se corta solo"
T_KILL=$(date +%s)
kill $CPPID; wait $CPPID 2>/dev/null
for _ in $(seq 1 100); do [ "$(field b config.lease_active)" = false ] && break; sleep 1; done
CUT_B=$(( $(date +%s) - T_KILL ))
[ "$(field b config.lease_active)" = false ] && [ $CUT_B -le 72 ] && ok "B, sin control plane, se corta al caducar su lease en ${CUT_B}s" || bad "B no se cortó (${CUT_B}s)"
[ "$(field b sync.sync_errors)" -ge 1 ] && ok "B registra sync_errors" || bad "B no registra errores de sincronización"
start_cp
for _ in $(seq 1 30); do python3 -c "import urllib.request;urllib.request.urlopen('$CP_URL/healthz',timeout=2)" 2>/dev/null && break; sleep 1; done
for _ in $(seq 1 30); do [ "$(field b config.lease_active)" = true ] && break; sleep 1; done
expect_eq "al volver el control plane, B recupera una configuración vigente" "$(field b config.lease_active)" true

section "Limpieza"
atenea down --socket $LAB/a.sock; atenea down --socket $LAB/b.sock; sleep 1
kill $CPPID 2>/dev/null
echo
if [ $FAILS -eq 0 ]; then echo "E2E CONTROL PLANE: TODO OK"; else echo "E2E CONTROL PLANE: $FAILS fallo(s)"; fi
exit $FAILS
