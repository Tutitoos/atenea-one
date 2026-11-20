#!/usr/bin/env bash
# Copyright 2026 Tutitoos
# SPDX-License-Identifier: Apache-2.0
# E2E de ADR-031 dentro del contenedor del laboratorio (ver run-e2e.sh): una red con conexiones automáticas y SIN ninguna
# política. Control plane real + PostgreSQL + Dex + tres agentes reales en espacios de red. Comprueba que los equipos
# aprobados se conectan solos, que una exclusión por pareja corta solo esa pareja, que una política manual la solapa
# sin deshacerla y que la revocación sigue funcionando.
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
for n in a b c; do
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
atenea-control-plane > $LAB/cp.log 2>&1 & CPPID=$!
for _ in $(seq 1 30); do python3 -c "import urllib.request;urllib.request.urlopen('$CP_URL/healthz',timeout=2)" 2>/dev/null && break; sleep 1; done
check "control plane en /healthz" python3 -c "import urllib.request;urllib.request.urlopen('$CP_URL/healthz',timeout=2)"

section "Red nueva: conexiones automáticas por defecto y ninguna política"
python3 "$CP" login admin@example.test password > $LAB/login.txt 2>&1
expect_eq "inicio de sesión OIDC completado" "$(cat $LAB/login.txt)" admin@example.test
[ "$(cat $LAB/login.txt)" = admin@example.test ] || { tail -20 $LAB/cp.log; exit 1; }
ORG=$(api POST /api/v1/orgs --json '{"name":"lab"}' --field id)
atenea root-init --out $LAB/root.key > $LAB/root.pub
ROOT_PUB=$(cat $LAB/root.pub)
NETID=$(api POST /api/v1/orgs/$ORG/networks --json "{\"name\":\"lab-auto\",\"ipv4_cidr\":\"10.78.0.0/24\",\"ipv6_cidr\":\"fd78::/64\",\"root_anchor\":\"$ROOT_PUB\"}" --field id)
[ -n "$NETID" ] && ok "red creada ($NETID)" || { bad "no se creó la red"; exit 1; }
expect_eq "la red nueva nace con conexiones automáticas" "$(api GET /api/v1/networks/$NETID --field auto_connect)" true
NETHEX=${NETID//-/}
atenea issuer-cert --root $LAB/root.key --issuer-key $LAB/issuer.key --network-id $NETHEX --serial 1 --valid-days 30 --out $LAB/issuer.cert
check "certificado de emisor subido" api PUT /api/v1/networks/$NETID/issuer-cert --file $LAB/issuer.cert

enroll() {  # enroll <ns>: claves, token, alta y aprobación; deja el id en DEV_<NS>
  local n=$1
  atenea device-init --dir $LAB/$n > $LAB/$n.info
  api POST /api/v1/networks/$NETID/enrollment-tokens --json '{"ttl_secs":600}' > $LAB/$n.token.json
  python3 -c "import json;print(json.dumps(json.load(open('$LAB/$n.token.json'))['join']))" > $LAB/$n.join.json
  atenea enroll --url $CP_URL --token "$(python3 -c "import json;print(json.load(open('$LAB/$n.token.json'))['token'])")" --dir $LAB/$n --name host-$n --insecure-http > $LAB/$n.enroll.txt 2>&1
  local id; id=$(uuid_of "$(sed -n 's/^device_id=//p' $LAB/$n.info)")
  api POST /api/v1/networks/$NETID/devices/$id/approve > /dev/null && ok "$n aprobado" || bad "no se aprobó $n"
  api PATCH /api/v1/networks/$NETID/devices/$id --json "{\"endpoints\":[\"192.0.2.$2:51820\"]}" >/dev/null
  eval "DEV_$(echo $n | tr a-z A-Z)=$id"
}
start_agent() {
  local n=$1
  atenea agent-config --join "$(cat $LAB/$n.join.json)" --dir $LAB/$n --out $LAB/$n.toml --state-dir $LAB/$n.state --config-file $LAB/$n.cfg --ipc-socket $LAB/$n.sock --insecure-http > /dev/null
  echo "interval_secs = 10" >> $LAB/$n.toml
  ip netns exec $n atenea up --config $LAB/$n.toml --log $LAB/$n.log > /dev/null && ok "agente $n arrancado" || { bad "arranque de $n"; cat $LAB/$n.log; }
  for _ in $(seq 1 30); do [ "$(field $n config.lease_active)" = true ] && break; sleep 1; done
  expect_eq "$n obtuvo una configuración vigente" "$(field $n config.lease_active)" true
  eval "IP_$(echo $n | tr a-z A-Z)=$(ip -n $n -4 -o addr show dev atenea0 | python3 -c "import sys,re;print(re.search(r'inet (\S+?)/',sys.stdin.read()).group(1))")"
}
reaches() { for _ in $(seq 1 25); do ip netns exec "$1" ping -c 1 -W 2 "$2" >/dev/null 2>&1 && return 0; done; return 1; }
unreachable() { for _ in $(seq 1 3); do ip netns exec "$1" ping -c 2 -W 2 "$2" >/dev/null 2>&1 && return 1; done; return 0; }
peers_of() { atenea status --socket "$LAB/$1.sock" | python3 -c "import json,sys;print(len(json.load(sys.stdin)['peers']))"; }
wait_peers() { for _ in $(seq 1 40); do [ "$(peers_of $1)" = "$2" ] && return 0; sleep 1; done; return 1; }
npol() { api GET /api/v1/networks/$NETID/policies | python3 -c "import json,sys;print(len(json.load(sys.stdin)))"; }

section "A y B se aprueban en ese orden: se conectan sin política"
enroll a 1; enroll b 2
start_agent a; start_agent b
expect_eq "no existe ninguna política en la red" "$(npol)" 0
check "ping IPv4 A→B sin política" reaches a $IP_B
check "ping IPv4 B→A sin política" reaches b $IP_A
ip netns exec b python3 "$TOOLS" tcp-server $IP_B 9000 > $LAB/srv.json &
S=$!
ip netns exec a python3 "$TOOLS" tcp-client $IP_B 9000 2000000 5 > $LAB/cli.json
wait $S
expect_eq "TCP 2 MB A→B íntegro (la conexión automática no filtra por puerto)" "$(python3 -c "import json;print(json.load(open('$LAB/srv.json'))['sha256'])")" "$(python3 -c "import json;print(json.load(open('$LAB/cli.json'))['sha256'])")"
expect_eq "A tiene un par" "$(peers_of a)" 1

section "C se aprueba después: se conecta con A y con B"
enroll c 3
start_agent c
check "C alcanza a A" reaches c $IP_A
check "C alcanza a B" reaches c $IP_B
check "A alcanza a C" reaches a $IP_C
check "B alcanza a C" reaches b $IP_C
wait_peers a 2 && ok "A tiene dos pares" || bad "A no recibió a C como par"
expect_eq "sigue sin existir ninguna política" "$(npol)" 0
AUTO_A=$(api GET /api/v1/networks/$NETID/devices/$DEV_A/auto-peers | python3 -c "import json,sys;print(' '.join(sorted(json.load(sys.stdin))))")
expect_eq "el control plane lista a B y C como conexiones automáticas de A" "$AUTO_A" "$(python3 -c "import sys;print(' '.join(sorted(sys.argv[1:])))" $DEV_B $DEV_C)"

section "Exclusión de la pareja A–C"
api DELETE /api/v1/networks/$NETID/devices/$DEV_A/auto-peers/$DEV_C >/dev/null && ok "exclusión creada" || bad "no se creó la exclusión"
wait_peers a 1 && wait_peers c 1 && ok "A y C pasan a tener un solo par cada uno" || bad "los pares no se actualizaron"
check "A ya no alcanza a C" unreachable a $IP_C
check "C ya no alcanza a A" unreachable c $IP_A
check "A sigue alcanzando a B" reaches a $IP_B
check "C sigue alcanzando a B" reaches c $IP_B
check "B sigue alcanzando a A y a C" bash -c "ip netns exec b ping -c 1 -W 2 $IP_A >/dev/null && ip netns exec b ping -c 1 -W 2 $IP_C >/dev/null"

section "Una política manual A–C solapa la exclusión y no la deshace"
api POST /api/v1/networks/$NETID/policies --json "{\"src\":{\"kind\":\"device\",\"device_id\":\"$DEV_A\"},\"dst\":{\"kind\":\"device\",\"device_id\":\"$DEV_C\"},\"proto\":1}" >/dev/null && ok "política ICMP manual A–C" || bad "no se creó la política"
check "con la política manual A alcanza a C" reaches a $IP_C
check "con la política manual C alcanza a A (regla inversa)" reaches c $IP_A
POL=$(api GET /api/v1/networks/$NETID/policies | python3 -c "import json,sys;print(json.load(sys.stdin)[0]['id'])")
api DELETE /api/v1/networks/$NETID/policies/$POL >/dev/null && ok "política manual borrada" || bad "no se borró la política"
for _ in $(seq 1 40); do unreachable a $IP_C && break; sleep 1; done
check "al borrarla vuelve a valer la exclusión: A no alcanza a C" unreachable a $IP_C
expect_eq "la exclusión sigue registrada tras borrar la política (A conserva solo a B)" "$(api GET /api/v1/networks/$NETID/devices/$DEV_A/auto-peers | python3 -c 'import json,sys;print(len(json.load(sys.stdin)))')" 1

section "Exclusión persistente: C se reinicia y la pareja sigue cortada"
atenea down --socket $LAB/c.sock >/dev/null 2>&1; sleep 2
ip netns exec c atenea up --config $LAB/c.toml --log $LAB/c.log >/dev/null
for _ in $(seq 1 30); do [ "$(field c config.lease_active)" = true ] && break; sleep 1; done
check "C vuelve a tener lease" test "$(field c config.lease_active)" = true
check "tras reiniciar, C alcanza a B" reaches c $IP_B
check "tras reiniciar, C sigue sin alcanzar a A" unreachable c $IP_A

section "Revocación de B"
T_REV=$(date +%s)
api POST /api/v1/networks/$NETID/devices/$DEV_B/revoke >/dev/null && ok "B revocado" || bad "revocación"
FAILS_IN_ROW=0
while [ $(( $(date +%s) - T_REV )) -lt 40 ]; do
  if ip netns exec a ping -c 1 -W 1 $IP_B >/dev/null 2>&1; then FAILS_IN_ROW=0; else FAILS_IN_ROW=$((FAILS_IN_ROW + 1)); fi
  [ $FAILS_IN_ROW -ge 2 ] && break
  sleep 0.5
done
CUT=$(( $(date +%s) - T_REV ))
[ $FAILS_IN_ROW -ge 2 ] && [ $CUT -le 20 ] && ok "A deja de alcanzar a B en ${CUT}s" || bad "A sigue alcanzando a B (${CUT}s)"
check "C tampoco alcanza ya a B" unreachable c $IP_B
wait_peers a 0 && wait_peers c 0 && ok "A y C se quedan sin pares" || bad "A o C conservan pares"
expect_eq "sigue sin existir ninguna política" "$(npol)" 0

section "Limpieza"
for n in a b c; do atenea down --socket $LAB/$n.sock >/dev/null 2>&1; done; sleep 1
kill $CPPID 2>/dev/null
echo
if [ $FAILS -eq 0 ]; then echo "E2E CONEXIONES AUTOMÁTICAS: TODO OK"; else echo "E2E CONEXIONES AUTOMÁTICAS: $FAILS fallo(s)"; fi
exit $FAILS
