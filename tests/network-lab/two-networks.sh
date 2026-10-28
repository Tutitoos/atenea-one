#!/usr/bin/env bash
# Copyright 2026 Tutitoos
# SPDX-License-Identifier: Apache-2.0
# ADR-013 fase 2 dentro del contenedor del laboratorio (ver run-two-networks.sh).
# Un cliente (namespace "cli") unido con `atenea network join` a dos redes del mismo control plane
# (A con código de token, B con código sin token y política con_solicitud) y un peer por red (pa, pb).
set -uo pipefail

LAB=/tmp/lab
ROOT=$LAB/networks      # raíz de redes del cliente (ATENEA_NETWORKS_DIR)
RUN=$LAB/run            # directorio de sockets del cliente (ATENEA_RUN_DIR)
TOOLS=$PWD/tests/network-lab/tools.py
CP=$PWD/tests/network-lab/cp.py
export CP_URL=http://192.0.2.254:8080 CP_JAR=$LAB/cp.cookies
FAILS=0
ok()   { echo "  OK   $*"; }
bad()  { echo "  FAIL $*"; FAILS=$((FAILS + 1)); }
check() { local d=$1; shift; if "$@" >/dev/null 2>&1; then ok "$d"; else bad "$d"; fi; }
expect_eq() { if [ -n "$2" ] && [ "$2" = "$3" ]; then ok "$1 ($2)"; else bad "$1: esperado '$3', obtenido '$2'"; fi; }
expect_ne() { if [ -n "$2" ] && [ -n "$3" ] && [ "$2" != "$3" ]; then ok "$1"; else bad "$1: '$2' y '$3' no son distintos y no vacíos"; fi; }
api()   { python3 "$CP" api "$@"; }
section() { echo; echo "== $*"; }
uuid_of() { python3 -c "import uuid,sys;print(uuid.UUID(sys.argv[1]))" "$1"; }

# --- cliente (todo `atenea network` se ejecuta dentro del namespace cli) ---
cli() { ip netns exec cli env ATENEA_NETWORKS_DIR="$ROOT" ATENEA_RUN_DIR="$RUN" atenea "$@"; }
# lj <red> <campo>: campo de `network list --json` para esa red
lj() { cli network list --json | python3 -c 'import sys,json;r=[x for x in json.load(sys.stdin) if x["network_id"]==sys.argv[1]];print(r[0].get(sys.argv[2],"") if r else "")' "$1" "$2"; }
id8() { local h=${1//-/}; echo "${h: -8}"; }  # últimos 8 hex: los UUIDv7 comparten el prefijo temporal
sfield() { python3 "$TOOLS" field "$RUN/$(id8 "$1")/status.sock" "$2" 2>/dev/null; }
kv() { sed -n "s/^$2=//;T;p;q" "$1"; }
wait_state() {  # wait_state <red> <estado> <segundos>
  local _; for _ in $(seq 1 "$3"); do [ "$(lj "$1" state)" = "$2" ] && return 0; sleep 1; done; return 1
}
# Huella de los ficheros estáticos de una red (el estado que el agente reescribe se excluye a propósito).
static_hash() { (cd "$ROOT/$1" && sha256sum network.json device/agent.toml device/noise.key device/identity.key device/bundle.bin | sha256sum | cut -c1-64); }
# Recursos de una red en el cliente: direcciones, rutas, puerto UDP y sockets (inodos).
res_sig() {
  local n=$1 iface port
  iface=$(lj "$n" interface); port=$(lj "$n" listen_port)
  ip -n cli -o addr show dev "$iface"; ip -n cli route show dev "$iface"
  ip netns exec cli ss -ulnH "sport = :$port"
  stat -c '%i' "$RUN/$(id8 "$n")/control.sock" "$RUN/$(id8 "$n")/status.sock"
}
set_interval() {  # sondeo del agente cada 10 s (como e2e-cp.sh), dentro de [sync]
  python3 - "$1" <<'PY'
import sys
p = sys.argv[1]
lines = open(p).read().split("\n")
out, done = [], False
for l in lines:
    out.append(l)
    if l.strip() == "[sync]" and not done:
        out.append("interval_secs = 10"); done = True
if not done:
    sys.exit("agent.toml sin sección [sync]")
open(p, "w").write("\n".join(out))
PY
}
approve_fp() {  # approve_fp <red> <huella>: el admin aprueba comparando la huella; imprime el device_id
  local dev
  dev=$(api GET /api/v1/networks/$1/devices | python3 -c 'import sys,json;[print(d["id"]) for d in json.load(sys.stdin) if d["fingerprint"]==sys.argv[1] and d["status"]=="pending"]' "$2")
  [ -n "$dev" ] || return 1
  api POST /api/v1/networks/$1/devices/$dev/approve >/dev/null || return 1
  echo "$dev"
}
dev_field() { api GET /api/v1/networks/$1/devices | python3 -c 'import sys,json;[print(d[sys.argv[2]]) for d in json.load(sys.stdin) if d["id"]==sys.argv[1]]' "$2" "$3"; }
bgping() {  # bgping <ns> <ip> <salida>: ping continuo; devuelve el pid en BGPID
  ip netns exec "$1" ping -i 0.2 -q "$2" > "$3" 2>&1 & BGPID=$!
}
endping() {  # endping <pid> <salida> <descripción>: sin una sola pérdida y con ≥ 20 paquetes
  kill -INT "$1" 2>/dev/null; wait "$1" 2>/dev/null
  local sent; sent=$(sed -n 's/^\([0-9]*\) packets transmitted.*/\1/p' "$2")
  if [ "${sent:-0}" -ge 20 ] && grep -q ' 0% packet loss' "$2"; then ok "$3: 0 % de pérdida en $sent paquetes"; else bad "$3: pérdida o muestra insuficiente"; cat "$2"; fi
}
mknet() {  # mknet <org> <nombre> <v4> <v6>: red + raíz propia + certificado de emisor; deja NET_ID
  local org=$1 name=$2
  atenea root-init --out $LAB/root-$name.key > $LAB/root-$name.pub
  NET_ID=$(api POST /api/v1/orgs/$org/networks --json "{\"name\":\"$name\",\"ipv4_cidr\":\"$3\",\"ipv6_cidr\":\"$4\",\"root_anchor\":\"$(cat $LAB/root-$name.pub)\"}" --field id)
  [ -n "$NET_ID" ] || { bad "no se creó la red $name"; return 1; }
  atenea issuer-cert --root $LAB/root-$name.key --issuer-key $LAB/issuer.key --network-id ${NET_ID//-/} --serial 1 --valid-days 30 --out $LAB/issuer-$name.cert
  api PUT /api/v1/networks/$NET_ID/issuer-cert --file $LAB/issuer-$name.cert >/dev/null && ok "red $name creada ($NET_ID, $3, raíz propia)" || bad "certificado de emisor de $name"
  for proto in 1 58; do api POST /api/v1/networks/$NET_ID/policies --json "{\"src\":{\"kind\":\"any\"},\"dst\":{\"kind\":\"any\"},\"proto\":$proto}" >/dev/null; done
}
peer() {  # peer <ns> <red>: enrola (token), aprueba, fija endpoint y arranca un agente clásico; deja P_DEV
  local n=$1 net=$2
  atenea device-init --dir $LAB/$n > $LAB/$n.info
  api POST /api/v1/networks/$net/enrollment-tokens --json '{"ttl_secs":600}' > $LAB/$n.token.json
  python3 - "$LAB/$n.token.json" "$CP_URL" > $LAB/$n.join.json <<'PY'
import json, sys
j = json.load(open(sys.argv[1])); j["join"]["url"] = sys.argv[2]; print(json.dumps(j["join"]))
PY
  atenea enroll --url $CP_URL --token "$(python3 -c "import json;print(json.load(open('$LAB/$n.token.json'))['token'])")" --dir $LAB/$n --name host-$n --insecure-http > $LAB/$n.enroll.txt 2>&1
  P_DEV=$(uuid_of "$(sed -n 's/^device_id=//p' $LAB/$n.info)")
  api POST /api/v1/networks/$net/devices/$P_DEV/approve >/dev/null || bad "no se aprobó el peer $n"
  api PATCH /api/v1/networks/$net/devices/$P_DEV --json "{\"endpoints\":[\"192.0.2.$3:51820\"]}" >/dev/null
  atenea agent-config --join "$(cat $LAB/$n.join.json)" --dir $LAB/$n --out $LAB/$n.toml --state-dir $LAB/$n.state --config-file $LAB/$n.cfg --ipc-socket $LAB/$n.sock --insecure-http
  echo "interval_secs = 10" >> $LAB/$n.toml
  ip netns exec $n atenea up --config $LAB/$n.toml --log $LAB/$n.log >/dev/null || bad "arranque del peer $n"
}
# client_join <código> <nombre> <red>: une el cliente, compara huella, aprueba, fija endpoint y arranca. Deja J_FP, J_DEV.
client_join() {
  local code=$1 name=$2 net=$3
  cli network join "$code" --name "$name" --yes --insecure-http > $LAB/join-$name.txt 2>&1 || { bad "join $name"; cat $LAB/join-$name.txt; return 1; }
  expect_eq "join $name: red devuelta" "$(kv $LAB/join-$name.txt red)" "$net"
  J_FP=$(kv $LAB/join-$name.txt huella_dispositivo)
  expect_eq "join $name: estado tras la unión" "$(kv $LAB/join-$name.txt estado)" pendiente_de_aprobacion
  J_DEV=$(approve_fp $net "$J_FP") && ok "admin aprueba $name comparando la huella $J_FP" || { bad "el servidor no muestra pendiente la huella $J_FP"; return 1; }
  api PATCH /api/v1/networks/$net/devices/$J_DEV --json "{\"endpoints\":[\"192.0.2.1:$(lj $net listen_port)\"]}" >/dev/null || bad "endpoint de $name"
  set_interval "$ROOT/$net/device/agent.toml" || bad "agent.toml de $name sin [sync]"
  cli network up "$net" >/dev/null 2>&1 && ok "network up $name" || bad "network up $name"
}

section "Compilación"
cargo build --release --locked -q 2>&1 | tail -3
BIN=$CARGO_TARGET_DIR/release
export PATH=$BIN:$PATH
rm -rf "$LAB"; mkdir -p "$LAB" "$RUN"

section "Infraestructura: PostgreSQL, Dex, topología y control plane"
ADMIN_URL=postgres://postgres:pg@postgres/atenea
for _ in $(seq 1 60); do psql "$ADMIN_URL" -qc 'select 1' >/dev/null 2>&1 && break; sleep 1; done
check "PostgreSQL responde" psql "$ADMIN_URL" -qc 'select 1'
for _ in $(seq 1 60); do python3 -c "import urllib.request;urllib.request.urlopen('http://dex:5556/dex/.well-known/openid-configuration',timeout=2)" 2>/dev/null && break; sleep 1; done
check "Dex responde" python3 -c "import urllib.request;urllib.request.urlopen('http://dex:5556/dex/.well-known/openid-configuration',timeout=2)"

ip link add br0 type bridge && ip link set br0 up && ip addr add 192.0.2.254/24 dev br0
i=0
for n in cli pa pb; do
  i=$((i + 1))
  ip netns add $n
  ip link add va-$n type veth peer name v$n
  ip link set v$n netns $n
  ip link set va-$n master br0 up
  ip -n $n link set lo up
  ip -n $n addr add 192.0.2.$i/24 dev v$n
  ip -n $n link set v$n up
done
# El reenvío del kernel se activa a propósito: el aislamiento debe venir del agente, no de un sysctl apagado.
ip netns exec cli sysctl -qw net.ipv4.ip_forward=1 net.ipv6.conf.all.forwarding=1

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
start_cp() { atenea-control-plane > $LAB/cp.log 2>&1 & CPPID=$!; for _ in $(seq 1 30); do python3 -c "import urllib.request;urllib.request.urlopen('$CP_URL/healthz',timeout=2)" 2>/dev/null && break; sleep 1; done; }
start_cp
check "control plane en /healthz" python3 -c "import urllib.request;urllib.request.urlopen('$CP_URL/healthz',timeout=2)"
python3 "$CP" login admin@example.test password > $LAB/login.txt 2>&1
expect_eq "inicio de sesión OIDC (Code + PKCE) completado" "$(cat $LAB/login.txt)" admin@example.test
[ "$(cat $LAB/login.txt)" = admin@example.test ] || { tail -20 $LAB/cp.log; exit 1; }
# El código de red exige PUBLIC_URL https: tras el login (cuyo redirect final usa PUBLIC_URL) se reinicia el
# control plane con una URL https; la API del laboratorio sigue por http y el CLI sustituye el esquema (--insecure-http).
kill $CPPID; wait $CPPID 2>/dev/null
export ATENEA_CP_PUBLIC_URL=https://192.0.2.254:8080
start_cp
check "control plane de nuevo en /healthz con PUBLIC_URL https" python3 -c "import urllib.request;urllib.request.urlopen('$CP_URL/healthz',timeout=2)"

section "Redes: A y B (misma organización, CIDR distintos) y C solapada con A (otra organización)"
ORG=$(api POST /api/v1/orgs --json '{"name":"lab"}' --field id)
ORG2=$(api POST /api/v1/orgs --json '{"name":"lab-2"}' --field id)
mknet $ORG red-a 10.78.1.0/24 fd78:1::/64; NET_A=$NET_ID
mknet $ORG red-b 10.78.2.0/24 fd78:2::/64; NET_B=$NET_ID
# El servidor solo impide solapes dentro de una organización; el rechazo entre organizaciones es del cliente.
mknet $ORG2 red-c 10.78.1.128/25 fd78:3::/64; NET_C=$NET_ID
api PUT /api/v1/networks/$NET_B/join-policy --json '{"policy":"con_solicitud"}' >/dev/null && ok "B con política con_solicitud"
api PUT /api/v1/networks/$NET_C/join-policy --json '{"policy":"con_solicitud"}' >/dev/null && ok "C con política con_solicitud"
expect_eq "A sigue con política cerrada" "$(api GET /api/v1/networks/$NET_A --field join_policy)" cerrada

section "Peers: un agente clásico en cada red (pa en A, pb en B)"
peer pa $NET_A 2; DEV_PA=$P_DEV
peer pb $NET_B 3; DEV_PB=$P_DEV

section "Unión del cliente: código con token (A) y código sin token con solicitud (B)"
TOK_JSON=$(api POST /api/v1/networks/$NET_A/enrollment-tokens --json '{"ttl_secs":600}')
CODE_A=$(echo "$TOK_JSON" | python3 -c 'import sys,json;print(json.load(sys.stdin).get("code") or "")')
FPR_A=$(echo "$TOK_JSON" | python3 -c 'import sys,json;print(json.load(sys.stdin)["root_anchor_fingerprint"])')
[ -n "$CODE_A" ] && ok "el servidor emite un código con token para A" || bad "código de A nulo (PUBLIC_URL o nombre no válidos)"
JC_B=$(api GET /api/v1/networks/$NET_B/join-code)
CODE_B=$(echo "$JC_B" | python3 -c 'import sys,json;print(json.load(sys.stdin)["code"])')
FPR_B=$(echo "$JC_B" | python3 -c 'import sys,json;print(json.load(sys.stdin)["root_anchor_fingerprint"])')
[ -n "$CODE_B" ] && ok "el servidor emite un código sin token para B" || bad "sin código de B"
# A primero: los puertos se asignan desde 51820 en orden de unión.
client_join "$CODE_A" cli-a $NET_A; DEV_CA=$J_DEV; FP_CA=$J_FP
client_join "$CODE_B" cli-b $NET_B; DEV_CB=$J_DEV; FP_CB=$J_FP
expect_eq "el dispositivo de A entró con token" "$(dev_field $NET_A $DEV_CA join_origin)" token
expect_eq "el dispositivo de B entró por solicitud" "$(dev_field $NET_B $DEV_CB join_origin)" request
expect_eq "huella de raíz mostrada por el CLI para A = la del servidor" "$(lj $NET_A root_anchor_fingerprint)" "$FPR_A"
expect_eq "huella de raíz mostrada por el CLI para B = la del servidor" "$(lj $NET_B root_anchor_fingerprint)" "$FPR_B"
wait_state $NET_A conectada 60 && ok "red A conectada" || bad "red A no llegó a conectada ($(lj $NET_A state): $(lj $NET_A detail))"
wait_state $NET_B conectada 60 && ok "red B conectada" || bad "red B no llegó a conectada ($(lj $NET_B state): $(lj $NET_B detail))"
for _ in $(seq 1 30); do [ "$(python3 "$TOOLS" field $LAB/pa.sock config.lease_active)" = true ] && [ "$(python3 "$TOOLS" field $LAB/pb.sock config.lease_active)" = true ] && break; sleep 1; done

section "Metadatos y recursos propios de cada red"
IF_A=$(lj $NET_A interface); IF_B=$(lj $NET_B interface); PORT_A=$(lj $NET_A listen_port); PORT_B=$(lj $NET_B listen_port)
for n in $NET_A $NET_B; do
  check "network.json de ${n:0:8} con los campos del contrato" python3 -c "
import json,sys
m=json.load(open('$ROOT/$n/network.json'))
assert m['network_id']=='$n' and m['device_id']=='$(lj $n device_id)' and m['device_fingerprint']=='$(lj $n device_fingerprint)'
for k in ('name','server','url','root_anchor_fingerprint','interface','listen_port','created','join_status'): assert m[k] not in (None,'')
assert 'token' not in json.dumps(m).lower()"
  check "agent.toml y claves de ${n:0:8} en device/" bash -c "cd $ROOT/$n/device && test -s agent.toml && test -s noise.key && test -s identity.key && test -s bundle.bin"
done
echo "       interfaces: A=$IF_A B=$IF_B   puertos: A=$PORT_A B=$PORT_B"
expect_ne "interfaces distintas" "$IF_A" "$IF_B"
expect_ne "puertos UDP distintos" "$PORT_A" "$PORT_B"
expect_eq "primer puerto de la serie (51820)" "$PORT_A" 51820
expect_eq "segundo puerto de la serie (51821)" "$PORT_B" 51821
check "interfaz $IF_A existe en el cliente" ip -n cli link show dev $IF_A
check "interfaz $IF_B existe en el cliente" ip -n cli link show dev $IF_B
check "UDP $PORT_A escuchando en el cliente" bash -c "ip netns exec cli ss -ulnH 'sport = :$PORT_A' | grep -q ."
check "UDP $PORT_B escuchando en el cliente" bash -c "ip netns exec cli ss -ulnH 'sport = :$PORT_B' | grep -q ."
for n in $NET_A $NET_B; do
  for s in control status; do check "socket $(id8 $n)/$s.sock existe" test -S "$RUN/$(id8 $n)/$s.sock"; done
done
expect_ne "sockets de control distintos" "$(stat -c %i $RUN/$(id8 $NET_A)/control.sock)" "$(stat -c %i $RUN/$(id8 $NET_B)/control.sock)"
expect_ne "id8 distintos" "$(id8 $NET_A)" "$(id8 $NET_B)"
expect_eq "A obtuvo una configuración vigente" "$(sfield $NET_A config.lease_active)" true
expect_eq "B obtuvo una configuración vigente" "$(sfield $NET_B config.lease_active)" true

section "Claves distintas por red"
expect_ne "clave Noise distinta" "$(sha256sum < $ROOT/$NET_A/device/noise.key)" "$(sha256sum < $ROOT/$NET_B/device/noise.key)"
expect_ne "clave de identidad distinta" "$(sha256sum < $ROOT/$NET_A/device/identity.key)" "$(sha256sum < $ROOT/$NET_B/device/identity.key)"
expect_ne "huella de dispositivo distinta" "$FP_CA" "$FP_CB"
expect_ne "bundle distinto" "$(sha256sum < $ROOT/$NET_A/device/bundle.bin)" "$(sha256sum < $ROOT/$NET_B/device/bundle.bin)"
expect_ne "raíz de confianza distinta" "$FPR_A" "$FPR_B"
OLD_NOISE_A=$(sha256sum < $ROOT/$NET_A/device/noise.key)

section "Ping en ambas redes"
IP_PA=$(dev_field $NET_A $DEV_PA ipv4); IP_PB=$(dev_field $NET_B $DEV_PB ipv4)
IP_CA=$(dev_field $NET_A $DEV_CA ipv4); IP_CB=$(dev_field $NET_B $DEV_CB ipv4)
echo "       IPAM: cliente A=$IP_CA B=$IP_CB   peers pa=$IP_PA pb=$IP_PB"
for _ in $(seq 1 30); do ip netns exec cli ping -c 1 -W 2 $IP_PA >/dev/null 2>&1 && ip netns exec cli ping -c 1 -W 2 $IP_PB >/dev/null 2>&1 && break; sleep 1; done
check "ping cliente → pa por A" ip netns exec cli ping -c 3 -W 3 $IP_PA
check "ping cliente → pb por B" ip netns exec cli ping -c 3 -W 3 $IP_PB
check "ping pa → cliente por A" ip netns exec pa ping -c 3 -W 3 $IP_CA
check "ping pb → cliente por B" ip netns exec pb ping -c 3 -W 3 $IP_CB

section "Aislamiento: el cliente no reenvía entre A y B"
expect_eq "reenvío IPv4 del kernel activo en el cliente (el aislamiento no depende del sysctl)" "$(ip netns exec cli sysctl -n net.ipv4.ip_forward)" 1
ip -n pa route add 10.78.2.0/24 dev atenea0 2>/dev/null; ip -n pb route add 10.78.1.0/24 dev atenea0 2>/dev/null
ip netns exec cli tcpdump -l -ni $IF_A icmp > $LAB/dump-a.txt 2>&1 & TA=$!
ip netns exec cli tcpdump -l -ni $IF_B icmp > $LAB/dump-b.txt 2>&1 & TB=$!
sleep 1
check "pa no alcanza a pb (ruta forzada por A hacia el cliente)" bash -c "! ip netns exec pa ping -c 3 -W 2 $IP_PB"
check "pb no alcanza a pa (ruta forzada por B hacia el cliente)" bash -c "! ip netns exec pb ping -c 3 -W 2 $IP_PA"
check "el cliente no alcanza a pb por la interfaz de A" bash -c "! ip netns exec cli ping -c 2 -W 2 -I $IF_A $IP_PB"
check "el cliente no alcanza a pa por la interfaz de B" bash -c "! ip netns exec cli ping -c 2 -W 2 -I $IF_B $IP_PA"
ip netns exec cli ping -c 2 -W 2 $IP_PA >/dev/null 2>&1
sleep 1; kill $TA $TB 2>/dev/null; wait $TA $TB 2>/dev/null
expect_eq "ningún paquete pa→pb entró por la interfaz de A del cliente" "$(grep -c "$IP_PA > $IP_PB" $LAB/dump-a.txt)" 0 || true
expect_eq "ningún paquete pa→pb salió por la interfaz de B del cliente" "$(grep -c "$IP_PA > $IP_PB" $LAB/dump-b.txt)" 0 || true
expect_eq "ningún paquete pb→pa en la interfaz de A del cliente" "$(grep -c "$IP_PB > $IP_PA" $LAB/dump-a.txt)" 0 || true
expect_eq "ningún paquete pb→pa en la interfaz de B del cliente" "$(grep -c "$IP_PB > $IP_PA" $LAB/dump-b.txt)" 0 || true
grep -q "$IP_CA > $IP_PA\|$IP_PA > $IP_CA" $LAB/dump-a.txt && ok "control positivo: la captura de A sí ve el ICMP cliente↔pa" || bad "control positivo: la captura de A no vio tráfico de A"
ip -n pa route del 10.78.2.0/24 2>/dev/null; ip -n pb route del 10.78.1.0/24 2>/dev/null

section "Código de red manipulado: error y nada en disco"
TAMPER=$(python3 -c "
import sys
c=sys.argv[1]; i=30
while c[i]=='-': i+=1
print(c[:i]+('A' if c[i]!='A' else 'B')+c[i+1:])" "$CODE_B")
BEFORE_ROOT=$(ls -A "$ROOT" | sort | tr '\n' ' '); BEFORE_LINKS=$(ip -n cli -o link show | cut -d: -f2 | sort | tr '\n' ' ')
BEFORE_DEVS=$(api GET /api/v1/networks/$NET_B/devices | python3 -c 'import sys,json;print(len(json.load(sys.stdin)))')
cli network join "$TAMPER" --name manipulado --yes --insecure-http --root $LAB/tampered > $LAB/tamper.txt 2>&1
[ $? -ne 0 ] && ok "el código alterado termina con error" || bad "el código alterado fue aceptado"
check "ningún fichero creado en la raíz de prueba" bash -c "[ ! -e $LAB/tampered ] || [ -z \"\$(find $LAB/tampered -mindepth 1 | head -1)\" ]"
expect_eq "la raíz real no cambia" "$(ls -A "$ROOT" | sort | tr '\n' ' ')" "$BEFORE_ROOT"
expect_eq "no aparece ninguna interfaz" "$(ip -n cli -o link show | cut -d: -f2 | sort | tr '\n' ' ')" "$BEFORE_LINKS"
expect_eq "el servidor no recibió ninguna solicitud" "$(api GET /api/v1/networks/$NET_B/devices | python3 -c 'import sys,json;print(len(json.load(sys.stdin)))')" "$BEFORE_DEVS"

section "Red tercera con CIDR solapado: rechazada sin tocar A ni B"
H_A=$(static_hash $NET_A); H_B=$(static_hash $NET_B); S_A=$(res_sig $NET_A); S_B=$(res_sig $NET_B)
LINKS=$(ip -n cli -o link show | cut -d: -f2 | sort | tr '\n' ' ')
CODE_C=$(api GET /api/v1/networks/$NET_C/join-code --field code)
cli network join "$CODE_C" --name cli-c --yes --insecure-http > $LAB/join-cli-c.txt 2>&1 && ok "join de C acepta la solicitud (el CIDR solo se conoce con la configuración)" || { bad "join de C"; cat $LAB/join-cli-c.txt; }
FP_CC=$(kv $LAB/join-cli-c.txt huella_dispositivo)
DEV_CC=$(approve_fp $NET_C "$FP_CC") && ok "admin aprueba C" || bad "C no aparece pendiente"
IF_C=$(lj $NET_C interface); PORT_C=$(lj $NET_C listen_port)
api PATCH /api/v1/networks/$NET_C/devices/$DEV_CC --json "{\"endpoints\":[\"192.0.2.1:$PORT_C\"]}" >/dev/null
set_interval "$ROOT/$NET_C/device/agent.toml"
cli network up $NET_C >/dev/null 2>&1
wait_state $NET_C conflicto 60 && ok "C queda en estado conflicto" || bad "C no pasó a conflicto (estado: $(lj $NET_C state))"
case "$(lj $NET_C detail)" in *network_overlap*) ok "motivo: $(lj $NET_C detail)";; *) bad "motivo inesperado: $(lj $NET_C detail)";; esac
# El agente crea el TUN al arrancar (antes de recibir configuración); la guarda impide direcciones y rutas.
expect_eq "la interfaz de C ($IF_C) no tiene direcciones" "$(ip -n cli -o addr show dev $IF_C | grep -c inet)" 0
expect_eq "no hay rutas del CIDR de C en el cliente" "$(ip -n cli route show | grep -c '10.78.1.128/25')" 0
expect_eq "el único cambio de interfaces es el dispositivo TUN vacío de C" "$(ip -n cli -o link show | cut -d: -f2 | grep -v "^ $IF_C\$" | sort | tr '\n' ' ')" "$LINKS"
expect_eq "ficheros estáticos de A intactos" "$(static_hash $NET_A)" "$H_A"
expect_eq "ficheros estáticos de B intactos" "$(static_hash $NET_B)" "$H_B"
expect_eq "recursos de A intactos" "$(res_sig $NET_A)" "$S_A"
expect_eq "recursos de B intactos" "$(res_sig $NET_B)" "$S_B"
check "ping a pa sigue funcionando" ip netns exec cli ping -c 3 -W 3 $IP_PA
check "ping a pb sigue funcionando" ip netns exec cli ping -c 3 -W 3 $IP_PB
cli network leave $NET_C --yes >/dev/null 2>&1 && ok "leave de C" || bad "leave de C"
expect_eq "tras salir de C, recursos de A intactos" "$(res_sig $NET_A)" "$S_A"
expect_eq "tras salir de C, recursos de B intactos" "$(res_sig $NET_B)" "$S_B"

section "Salir de A no corta ni modifica B"
H_B=$(static_hash $NET_B); S_B=$(res_sig $NET_B)
bgping cli $IP_PB $LAB/ping-b-leave.txt; PB_PID=$BGPID
sleep 6
cli network leave $NET_A --yes > $LAB/leave-a.txt 2>&1 && ok "network leave A" || { bad "network leave A"; cat $LAB/leave-a.txt; }
sleep 6
endping $PB_PID $LAB/ping-b-leave.txt "ping a pb durante y después de salir de A"
check "el directorio de A desaparece" test ! -e "$ROOT/$NET_A"
check "la interfaz de A desaparece" bash -c "! ip -n cli link show dev $IF_A"
check "el puerto de A queda libre" bash -c "! ip netns exec cli ss -ulnH 'sport = :$PORT_A' | grep -q ."
check "los sockets de A desaparecen" bash -c "[ ! -S $RUN/$(id8 $NET_A)/control.sock ] && [ ! -S $RUN/$(id8 $NET_A)/status.sock ]"
expect_eq "ficheros estáticos de B intactos" "$(static_hash $NET_B)" "$H_B"
expect_eq "recursos de B (dirección, rutas, puerto, sockets) intactos" "$(res_sig $NET_B)" "$S_B"
expect_eq "B sigue con configuración vigente" "$(sfield $NET_B config.lease_active)" true
expect_eq "B sigue conectada en el listado" "$(lj $NET_B state)" conectada
check "pa ya no se alcanza desde el cliente" bash -c "! ip netns exec cli ping -c 2 -W 2 $IP_PA"
api POST /api/v1/networks/$NET_A/devices/$DEV_CA/revoke >/dev/null && ok "el admin revoca el dispositivo antiguo de A en el servidor"

section "Volver a unir A (claves nuevas) para probar la revocación de B"
TOK_JSON=$(api POST /api/v1/networks/$NET_A/enrollment-tokens --json '{"ttl_secs":600}')
CODE_A=$(echo "$TOK_JSON" | python3 -c 'import sys,json;print(json.load(sys.stdin).get("code") or "")')
client_join "$CODE_A" cli-a2 $NET_A; DEV_CA=$J_DEV
expect_ne "las claves de A tras volver a unir son nuevas" "$(sha256sum < $ROOT/$NET_A/device/noise.key)" "$OLD_NOISE_A"
wait_state $NET_A conectada 60 && ok "A de nuevo conectada" || bad "A no reconectó ($(lj $NET_A state))"
IF_A=$(lj $NET_A interface); PORT_A=$(lj $NET_A listen_port)
IP_CA=$(dev_field $NET_A $DEV_CA ipv4)
for _ in $(seq 1 30); do ip netns exec cli ping -c 1 -W 2 $IP_PA >/dev/null 2>&1 && break; sleep 1; done
check "ping cliente → pa tras volver a unir" ip netns exec cli ping -c 3 -W 3 $IP_PA

section "Revocar B no afecta a A (ADR-002: validez 40 s, tolerancia 30 s, sondeo cada 10 s)"
H_A=$(static_hash $NET_A); S_A=$(res_sig $NET_A)
bgping cli $IP_PA $LAB/ping-a-revoke.txt; PA_PID=$BGPID
sleep 3
T_REV=$(date +%s)
api POST /api/v1/networks/$NET_B/devices/$DEV_CB/revoke >/dev/null && ok "dispositivo del cliente revocado en B" || bad "revocación en B"
for _ in $(seq 1 100); do [ "$(sfield $NET_B config.lease_active)" = false ] && break; sleep 1; done
CUT_B=$(( $(date +%s) - T_REV ))
[ "$(sfield $NET_B config.lease_active)" = false ] && [ $CUT_B -le 72 ] && ok "el cliente corta B al caducar su configuración en ${CUT_B}s" || bad "B no se cortó a tiempo (${CUT_B}s)"
check "pb ya no se alcanza desde el cliente" bash -c "! ip netns exec cli ping -c 2 -W 2 $IP_PB"
check "pb ya no responde a la dirección de B del cliente" bash -c "! ip netns exec pb ping -c 2 -W 2 $IP_CB"
endping $PA_PID $LAB/ping-a-revoke.txt "ping a pa durante toda la revocación de B"
expect_eq "A sigue con configuración vigente" "$(sfield $NET_A config.lease_active)" true
expect_eq "A sigue conectada en el listado" "$(lj $NET_A state)" conectada
expect_eq "ficheros estáticos de A intactos" "$(static_hash $NET_A)" "$H_A"
expect_eq "recursos de A (dirección, rutas, puerto, sockets) intactos" "$(res_sig $NET_A)" "$S_A"
check "ping pa → cliente sigue funcionando" ip netns exec pa ping -c 3 -W 3 $IP_CA

section "Limpieza"
cli network down $NET_A >/dev/null 2>&1; cli network down $NET_B >/dev/null 2>&1
atenea down --socket $LAB/pa.sock >/dev/null 2>&1; atenea down --socket $LAB/pb.sock >/dev/null 2>&1; sleep 1
kill $CPPID 2>/dev/null
echo
if [ $FAILS -eq 0 ]; then echo "DOS REDES: TODO OK"; else echo "DOS REDES: $FAILS fallo(s)"; fi
exit $FAILS
