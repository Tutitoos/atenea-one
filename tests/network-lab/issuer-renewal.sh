#!/usr/bin/env bash
# Copyright 2026 Tutitoos
# SPDX-License-Identifier: Apache-2.0
# Ensayo de la renovación del certificado de emisor (docs/operations/renovacion-certificado-emisor.md), dentro del
# contenedor del laboratorio (ver run-e2e.sh). Control plane real + PostgreSQL + Dex + un agente real.
# Raíz y emisor DESECHABLES, generados aquí: no toca ninguna clave ni red real.
set -uo pipefail

LAB=/tmp/lab
TOOLS=$PWD/tests/network-lab/tools.py
CP=$PWD/tests/network-lab/cp.py
export CP_URL=http://192.0.2.254:8080 CP_JAR=$LAB/cp.cookies
FAILS=0
ok()   { echo "  OK   $*"; }
bad()  { echo "  FAIL $*"; FAILS=$((FAILS + 1)); }
check() { local d=$1; shift; if "$@" >/dev/null 2>&1; then ok "$d"; else bad "$d"; fi; }
check_fail() { local d=$1; shift; if "$@" >/dev/null 2>&1; then bad "$d (se aceptó y debía rechazarse)"; else ok "$d"; fi; }
expect_eq() { if [ -n "$2" ] && [ "$2" = "$3" ]; then ok "$1 ($2)"; else bad "$1: esperado '$3', obtenido '$2'"; fi; }
api()   { python3 "$CP" api "$@"; }
field() { python3 "$TOOLS" field "$LAB/$1.sock" "$2"; }
ctr()   { python3 "$TOOLS" counter "$LAB/$1.sock" "$2"; }
section() { echo; echo "== $*"; }
days_left() {
  api GET /api/v1/networks/$NETID --field issuer_cert_not_after | python3 -c "
import sys, datetime
e = datetime.datetime.fromisoformat(sys.stdin.read().strip())
print((e - datetime.datetime.now(datetime.timezone.utc)).days)"
}
cert() {  # cert SERIAL DÍAS SALIDA [RAÍZ] [CLAVE_EMISOR] [RED_HEX]
  atenea issuer-cert --root "${4:-$LAB/root.key}" --issuer-key "${5:-$LAB/issuer.key}" --network-id "${6:-$NETHEX}" \
    --serial "$1" --valid-days "$2" --out "$3"
}
upload() { api PUT /api/v1/networks/$NETID/issuer-cert --file "$1"; }
# Muestrea el lease cada segundo durante N segundos; imprime cuántas veces estuvo caído.
lease_drops() { local n=0 i; for i in $(seq 1 "$1"); do [ "$(field a config.lease_active)" = true ] || n=$((n + 1)); sleep 1; done; echo $n; }

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
ip link add br0 type bridge && ip link set br0 up && ip addr add 192.0.2.254/24 dev br0
ip netns add a
ip link add va-a type veth peer name va
ip link set va netns a
ip link set va-a master br0 up
ip -n a link set lo up
ip -n a addr add 192.0.2.1/24 dev va
ip -n a link set va up
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
check "migraciones aplicadas" test $? -eq 0
export ATENEA_CP_DATABASE_URL=postgres://atenea_app:app-pass@postgres/atenea
atenea-control-plane > $LAB/cp.log 2>&1 & CPPID=$!
for _ in $(seq 1 30); do python3 -c "import urllib.request;urllib.request.urlopen('$CP_URL/healthz',timeout=2)" 2>/dev/null && break; sleep 1; done
check "control plane en /healthz" python3 -c "import urllib.request;urllib.request.urlopen('$CP_URL/healthz',timeout=2)"

section "Red con raíz y emisor desechables; certificado inicial (serial 1, 30 días)"
python3 "$CP" login admin@example.test password > $LAB/login.txt 2>&1
expect_eq "inicio de sesión OIDC" "$(cat $LAB/login.txt)" admin@example.test
ORG=$(api POST /api/v1/orgs --json '{"name":"renovacion"}' --field id)
atenea root-init --out $LAB/root.key > $LAB/root.pub
NETID=$(api POST /api/v1/orgs/$ORG/networks --json "{\"name\":\"red-renovacion\",\"ipv4_cidr\":\"10.77.0.0/24\",\"ipv6_cidr\":\"fd77::/64\",\"root_anchor\":\"$(cat $LAB/root.pub)\"}" --field id)
NETHEX=${NETID//-/}
[ -n "$NETID" ] && ok "red creada ($NETID)" || bad "no se creó la red"
cert 1 30 $LAB/s1.cert
check "certificado serial 1 subido" upload $LAB/s1.cert
D1=$(days_left)
check "el servidor informa su caducidad: ~30 días (obtenido $D1)" test "$D1" -ge 29 -a "$D1" -le 30

section "Un agente real obtiene configuración con el certificado serial 1"
atenea device-init --dir $LAB/a > $LAB/a.info
api POST /api/v1/networks/$NETID/enrollment-tokens --json '{"ttl_secs":600}' > $LAB/a.token.json
python3 -c "import json;print(json.dumps(json.load(open('$LAB/a.token.json'))['join']))" > $LAB/a.join.json
atenea enroll --url $CP_URL --token "$(python3 -c "import json;print(json.load(open('$LAB/a.token.json'))['token'])")" --dir $LAB/a --name host-a --insecure-http > $LAB/a.enroll.txt 2>&1
DEV_A=$(python3 -c "import uuid;print(uuid.UUID('$(sed -n 's/^device_id=//p' $LAB/a.info)'))")
api POST /api/v1/networks/$NETID/devices/$DEV_A/approve > /dev/null && ok "dispositivo aprobado" || bad "no se aprobó"
atenea agent-config --join "$(cat $LAB/a.join.json)" --dir $LAB/a --out $LAB/a.toml --state-dir $LAB/a.state --config-file $LAB/a.cfg --ipc-socket $LAB/a.sock --insecure-http
echo "interval_secs = 10" >> $LAB/a.toml
ip netns exec a atenea up --config $LAB/a.toml --log $LAB/a.log >/dev/null && ok "agente arrancado" || { bad "arranque"; cat $LAB/a.log; exit 1; }
for _ in $(seq 1 30); do [ "$(field a config.lease_active)" = true ] && break; sleep 1; done
expect_eq "lease activo con el certificado serial 1" "$(field a config.lease_active)" true
V0=$(field a config.version)

section "Renovación: mismo emisor, serial 2, 90 días (procedimiento del runbook)"
cert 2 90 $LAB/s2.cert
check "certificado serial 2 subido (204)" upload $LAB/s2.cert
D2=$(days_left)
check "el servidor informa la nueva caducidad: ~90 días (obtenido $D2)" test "$D2" -ge 89 -a "$D2" -le 90
check "la auditoría registra network.issuer_cert con serial 2" bash -c "python3 $CP api GET '/api/v1/networks/$NETID/audit?limit=50' | python3 -c \"import sys,json; e=[x for x in json.load(sys.stdin) if x['action']=='network.issuer_cert']; assert [x['detail']['serial'] for x in e][:2]==[2,1], e\""
DROPS=$(lease_drops 30)
expect_eq "el lease no cayó ni una vez durante la renovación (30 s muestreados)" "$DROPS" 0
for _ in $(seq 1 30); do [ "$(field a config.version)" != "$V0" ] && break; sleep 1; done
check "el agente recibió una configuración nueva firmada con el serial 2" test "$(field a config.version)" != "$V0"
expect_eq "sin errores de configuración tras la renovación" "$(field a config.last_error)" none
check "subir de nuevo el mismo serial 2 es idempotente" upload $LAB/s2.cert

section "Rechazos del servidor (no deben cambiar el certificado vigente)"
cert 3 30 $LAB/otra-red.cert "" "" 00112233445566778899aabbccddeeff
check_fail "certificado de OTRA red: rechazado" upload $LAB/otra-red.cert
atenea root-init --out $LAB/root2.key >/dev/null
cert 3 30 $LAB/otra-raiz.cert $LAB/root2.key
check_fail "certificado firmado por OTRA raíz: rechazado" upload $LAB/otra-raiz.cert
python3 -c "import secrets;print(secrets.token_hex(32))" > $LAB/issuer2.key; chmod 600 $LAB/issuer2.key
cert 3 30 $LAB/otro-emisor.cert "" $LAB/issuer2.key
MSG=$(upload $LAB/otro-emisor.cert 2>&1 || true)
case "$MSG" in *wrong_issuer*) ok "certificado de OTRO emisor: rechazado con wrong_issuer";; *) bad "otro emisor: respuesta inesperada: $MSG";; esac
check "tras los rechazos sigue la caducidad de ~90 días" test "$(days_left)" -ge 89

section "RIESGO: el servidor acepta un serial MENOR; el agente lo rechaza (anti-retroceso)"
S0=$(field a sync.sync_errors)
check "el servidor acepta subir el certificado serial 1 (no comprueba el orden)" upload $LAB/s1.cert
for _ in $(seq 1 40); do [ "$(field a sync.last_error)" = issuer_rollback ] && break; sleep 1; done
expect_eq "el agente rechaza el serial 1 y lo informa como sync.last_error" "$(field a sync.last_error)" issuer_rollback
check "los errores de sincronización del agente aumentan" test "$(field a sync.sync_errors)" -gt "$S0"
for _ in $(seq 1 100); do [ "$(field a config.lease_active)" = false ] && break; sleep 1; done
expect_eq "el agente se queda sin lease (falla cerrado) al caducar la última configuración válida" "$(field a config.lease_active)" false

section "Recuperación: volver a subir el certificado serial 2"
check "certificado serial 2 subido de nuevo" upload $LAB/s2.cert
for _ in $(seq 1 60); do [ "$(field a config.lease_active)" = true ] && break; sleep 1; done
expect_eq "el agente recupera el lease sin tocarlo" "$(field a config.lease_active)" true
for _ in $(seq 1 30); do [ "$(field a sync.last_error)" = none ] && break; sleep 1; done
expect_eq "el error issuer_rollback desaparece del estado del agente" "$(field a sync.last_error)" none

section "Limpieza"
atenea down --socket $LAB/a.sock; sleep 1
kill $CPPID 2>/dev/null
echo
if [ $FAILS -eq 0 ]; then echo "ENSAYO DE RENOVACIÓN DEL EMISOR: TODO OK"; else echo "ENSAYO DE RENOVACIÓN DEL EMISOR: $FAILS fallo(s)"; fi
exit $FAILS
