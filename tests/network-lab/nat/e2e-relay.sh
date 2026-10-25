#!/usr/bin/env bash
# E2E de la fase 5 (dentro del contenedor, ver run-e2e-relay.sh): agentes + coordinator + relay reales
# (UDP 443 y TLS/TCP 443 detrás de HAProxy por SNI/ALPN) tras los NAT de lib.sh. Fuerza cada camino
# con los bloqueadores del laboratorio y comprueba conmutación, integridad, ausencia de texto claro
# y tiempos. Claves y certificados se generan en cada ejecución.
set -uo pipefail

LAB=/tmp/lab
ROOT=$PWD
TOOLS=$ROOT/tests/network-lab/tools.py
FAILS=0
ok()   { echo "  OK   $*"; }
bad()  { echo "  FAIL $*"; FAILS=$((FAILS + 1)); }
section() { echo; echo "== $*"; }
cnt() { local v; v=$(field "$1" "counters.$2"); [ "$v" = error ] && v=0; echo "$v"; }
field() { python3 "$TOOLS" field "$LAB/$1.sock" "$2" 2>/dev/null || echo error; }
expect_eq() { if [ -n "$2" ] && [ "$2" = "$3" ]; then ok "$1 ($2)"; else bad "$1: esperado '$3', obtenido '$2'"; fi; }

section "Compilación"
cargo build --release --locked -q 2>&1 | tail -3
export PATH=$CARGO_TARGET_DIR/release:$PATH
source "$ROOT/tests/network-lab/nat/lib.sh"
trap 'topo_teardown 2>/dev/null; kill $(jobs -p) 2>/dev/null' EXIT

rm -rf "$LAB"; mkdir -p "$LAB"
NET=00112233445566778899aabbccddeeff
VALID=240; REFRESH=20

atenea root-init --out $LAB/root.key > $LAB/root.pub
ROOT_PUB=$(cat $LAB/root.pub)
atenea issuer-init --root $LAB/root.key --network-id $NET --serial 1 --valid-days 30 --out-dir $LAB/issuer > /dev/null
for n in a b; do atenea device-init --dir $LAB/$n > $LAB/$n.info; done
devid() { sed -n 's/^device_id=//p' $LAB/$1.info; }
bash tests/network-lab/nat/gen-certs.sh $LAB/certs > /dev/null

# Clave de firma de tokens (solo en el coordinator) y su pública para el relay
( umask 077; head -c 32 /dev/urandom | od -An -tx1 | tr -d ' \n' > $LAB/relay-signing.hex )
export ATENEA_COORD_RELAY_SIGNING_KEY_FILE=$LAB/relay-signing.hex
RELAY_PUB=$(atenea-coordinator --print-relay-key)

spec() {
  cat > $LAB/$1.spec.toml <<EOF
network_id = "$NET"
device_bundle = "$LAB/$1/bundle.bin"
validity_secs = $VALID
tunnel_addrs = ["10.77.0.$2", "fd77::$2"]
network_prefixes = ["10.77.0.0/24", "fd77::/64"]
[[peer]]
bundle = "$LAB/$3/bundle.bin"
addrs = ["10.77.0.$4", "fd77::$4"]
[[rule]]
src = "any"
dst = "any"
EOF
}
issue() { atenea issue --issuer-key $LAB/issuer/issuer.key --issuer-cert $LAB/issuer/issuer.cert --spec $LAB/$1.spec.toml --out $LAB/$1.cfg 2>/dev/null; }
spec a 1 b 2; spec b 2 a 1

agentcfg() {
  cat > $LAB/$1.toml <<EOF
interface = "atenea0"
listen = "0.0.0.0:51820"
ipc_socket = "$LAB/$1.sock"
noise_key_file = "$LAB/$1/noise.key"
identity_key_file = "$LAB/$1/identity.key"
config_file = "$LAB/$1.cfg"
state_dir = "$LAB/$1.state"
network_id = "$NET"
device_id = "$(devid $1)"
trust_anchor = "$ROOT_PUB"

[coordinator]
url = "http://198.51.100.10:8081"
insecure_http = true
stun = ["198.51.100.10:3478", "198.51.100.10:3479"]
relay_ca_file = "$LAB/certs/ca.pem"
EOF
}

# Espera hasta que el camino de A y B sea $1 (máx. $2 s); imprime los segundos tardados o "no".
wait_path() {
  local want=$1 max=$2 t0=$SECONDS
  while [ $((SECONDS - t0)) -lt "$max" ]; do
    if ip netns exec a ping -c 1 -W 1 10.77.0.2 >/dev/null 2>&1 \
       && [ "$(field a peers.0.path)" = "$want" ] && [ "$(field b peers.0.path)" = "$want" ]; then
      echo $((SECONDS - t0)); return 0
    fi
    sleep 0.5
  done
  echo no; return 1
}

hs() { echo "$(cnt a handshakes_initiator)/$(cnt b handshakes_initiator)/$(cnt a handshakes_responder)/$(cnt b handshakes_responder)"; }

integrity() {  # integrity <etiqueta> <seed>
  ip netns exec b python3 "$TOOLS" tcp-server 10.77.0.2 9000 > $LAB/srv.json &
  local S=$!; sleep 0.5
  ip netns exec a python3 "$TOOLS" tcp-client 10.77.0.2 9000 1000000 $2 > $LAB/cli.json
  wait $S
  expect_eq "TCP 1 MB íntegro por $1" \
    "$(python3 -c "import json;print(json.load(open('$LAB/srv.json'))['sha256'])")" \
    "$(python3 -c "import json;print(json.load(open('$LAB/cli.json'))['sha256'])")"
}

start_services() {
  rm -rf $LAB/a.state $LAB/b.state $LAB/*.sock
  issue a; issue b
  agentcfg a; agentcfg b
  ATENEA_COORD_LISTEN="[::]:8081" ATENEA_COORD_STUN_BIND="::" ATENEA_COORD_ANCHORS="[\"$ROOT_PUB\"]" \
    ATENEA_COORD_RELAYS='[{"name":"lab","udp":"198.51.100.10:443","tls":"198.51.100.10:443","tls_name":"relay.atenea.one"}]' \
    ip netns exec inet atenea-coordinator > $LAB/coord.log 2>&1 &
  ATENEA_RELAY_UDP_LISTEN=0.0.0.0:443 ATENEA_RELAY_TCP_LISTEN=127.0.0.1:8443 \
    ATENEA_RELAY_COORDINATOR_KEYS="[\"$RELAY_PUB\"]" \
    ATENEA_RELAY_TLS_CERT_FILE=$LAB/certs/relay.pem ATENEA_RELAY_TLS_KEY_FILE=$LAB/certs/relay.key \
    ATENEA_RELAY_PROXY_PROTOCOL=true ATENEA_RELAY_STATUS_SOCKET=$LAB/relay.sock \
    ip netns exec inet atenea-relay > $LAB/relay.log 2>&1 &
  ip netns exec inet python3 tests/network-lab/nat/fake-web.py 8082 > /dev/null 2>&1 &
  sleep 1
  start_haproxy 8443 8082 || bad "no arranca HAProxy"
  ( while true; do issue a; issue b; sleep $REFRESH; done ) &
  REFRESHER=$!
  ip netns exec a atenea up --config $LAB/a.toml --log $LAB/a.log >/dev/null
  ip netns exec b atenea up --config $LAB/b.toml --log $LAB/b.log >/dev/null
}

stop_services() {
  atenea down --socket $LAB/a.sock >/dev/null 2>&1; atenea down --socket $LAB/b.sock >/dev/null 2>&1
  kill $REFRESHER 2>/dev/null
  topo_teardown
}

relay_status() { ip netns exec inet python3 - "$LAB/relay.sock" "$1" <<'PY'
import json, socket, sys
s = socket.socket(socket.AF_UNIX); s.connect(sys.argv[1])
d = json.loads(s.makefile().readline())
for k in sys.argv[2].split("."): d = d[k]
print(d)
PY
}

# ---------------------------------------------------------------- home: caminos forzados
section "Topología home: P2P directo"
topo_setup home || { bad "no se pudo construir home"; exit 1; }
start_services
T=$(wait_path direct 60) && ok "camino directo en ${T}s" || bad "sin camino directo (a=$(field a peers.0.path) b=$(field b peers.0.path))"
integrity "ruta directa" 1

section "Bloqueo de P2P: debe pasar a relay UDP"
ip netns exec inet tcpdump -i eth0 -n -w $LAB/inet.pcap >/dev/null 2>&1 &
TCPD=$!; sleep 1
SW0=$(cnt a path_switches); HS0=$(hs)
T0=$SECONDS
net_block_p2p
T=$(wait_path relay-udp 40) && ok "relay UDP tras ${T}s del bloqueo" || bad "no pasó a relay UDP (a=$(field a peers.0.path) b=$(field b peers.0.path))"
[ "$T" != no ] && [ "$T" -le 10 ] 2>/dev/null && ok "recuperación ≤ 10 s ($T s)" || bad "recuperación > 10 s ($T)"
integrity "relay UDP" 2
ip netns exec a python3 "$TOOLS" send-udp 10.77.0.2 9100 "ATENEA-MARCA-EN-CLARO-9f3c"
sleep 1
expect_eq "misma sesión Noise al cambiar de camino (sin nuevo handshake)" "$(hs)" "$HS0"
[ "$(relay_status traffic.udp_packets)" -gt 0 ] 2>/dev/null && ok "el relay reenvió paquetes UDP" || bad "el relay no reenvió UDP"

section "Bloqueo de todo UDP: debe pasar a relay TLS/TCP 443"
net_block_udp
T=$(wait_path relay-tls 60) && ok "relay TLS tras ${T}s del bloqueo" || bad "no pasó a relay TLS (a=$(field a peers.0.path) b=$(field b peers.0.path))"
integrity "relay TLS (HAProxy SNI/ALPN)" 3
[ "$(relay_status traffic.tcp_packets)" -gt 0 ] 2>/dev/null && ok "el relay reenvió tramas TCP" || bad "el relay no reenvió TCP"

kill $TCPD 2>/dev/null; sleep 0.5
expect_eq "texto claro en el pcap del servidor" "$(python3 "$TOOLS" contains $LAB/inet.pcap ATENEA-MARCA-EN-CLARO-9f3c)" 0

section "Desbloqueo: vuelta al directo sin oscilación"
net_unblock_all
SW1=$(cnt a path_switches)
T=$(wait_path direct 90) && ok "directo restaurado en ${T}s" || bad "no volvió al directo"
sleep 20
SW2=$(cnt a path_switches)
echo "       conmutaciones de A: $SW0 → $SW1 → $SW2"
[ "$((SW2 - SW1))" -le 2 ] && ok "sin oscilación tras recuperar el directo" || bad "oscilación: $((SW2 - SW1)) cambios en 20 s"
expect_eq "sin fallos de autenticación en A" "$(cnt a rx_auth_fail)" 0
integrity "ruta directa recuperada" 4
stop_services

# ---------------------------------------------------------------- NAT simétrico
section "NAT simétrico: sin P2P posible, ahora conecta por relay"
topo_setup symmetric || { bad "no se pudo construir symmetric"; exit 1; }
start_services
T=$(wait_path relay-udp 60) && ok "conectado por relay UDP en ${T}s" || bad "simétrico sin conectividad (a=$(field a peers.0.path) b=$(field b peers.0.path))"
integrity "relay UDP (NAT simétrico)" 5
stop_services

echo
if [ $FAILS -eq 0 ]; then echo "E2E RELAY: TODO OK"; else echo "E2E RELAY: $FAILS fallo(s)"; fi
exit $FAILS
