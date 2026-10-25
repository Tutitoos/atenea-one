#!/usr/bin/env bash
# E2E de la fase 4 (dentro del contenedor, ver run-e2e.sh): agentes reales + coordinator real tras
# las topologías NAT de lib.sh. Las configuraciones firmadas no llevan endpoints: la conexión
# directa solo puede salir de STUN + coordinator + hole punching.
set -uo pipefail

LAB=/tmp/lab
ROOT=$PWD
TOOLS=$ROOT/tests/network-lab/tools.py
FAILS=0
ok()   { echo "  OK   $*"; }
bad()  { echo "  FAIL $*"; FAILS=$((FAILS + 1)); }
section() { echo; echo "== $*"; }
field() { python3 "$TOOLS" field "$LAB/$1.sock" "$2" 2>/dev/null || echo error; }
expect_eq() { if [ -n "$2" ] && [ "$2" = "$3" ]; then ok "$1 ($2)"; else bad "$1: esperado '$3', obtenido '$2'"; fi; }

section "Compilación"
cargo build --release --locked -q 2>&1 | tail -3
export PATH=$CARGO_TARGET_DIR/release:$PATH
source "$ROOT/tests/network-lab/nat/lib.sh"
trap 'topo_teardown 2>/dev/null' EXIT

rm -rf "$LAB"; mkdir -p "$LAB"
NET=00112233445566778899aabbccddeeff
VALID=240; REFRESH=20

# Jerarquía de confianza y dispositivos (una vez; las topologías se rehacen)
atenea root-init --out $LAB/root.key > $LAB/root.pub
ROOT_PUB=$(cat $LAB/root.pub)
atenea issuer-init --root $LAB/root.key --network-id $NET --serial 1 --valid-days 30 --out-dir $LAB/issuer > /dev/null
for n in a b; do atenea device-init --dir $LAB/$n > $LAB/$n.info; done
devid() { sed -n 's/^device_id=//p' $LAB/$1.info; }

spec() {  # spec <ns> <idx> <peer_ns> <peer_idx>   (sin endpoints: nada fijo, todo por coordinator)
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

agentcfg() {  # agentcfg <ns> <listen> <coord_url> <stun_host>
  cat > $LAB/$1.toml <<EOF
interface = "atenea0"
listen = "$2"
ipc_socket = "$LAB/$1.sock"
noise_key_file = "$LAB/$1/noise.key"
identity_key_file = "$LAB/$1/identity.key"
config_file = "$LAB/$1.cfg"
state_dir = "$LAB/$1.state"
network_id = "$NET"
device_id = "$(devid $1)"
trust_anchor = "$ROOT_PUB"

[coordinator]
url = "$3"
insecure_http = true
stun = ["$4:3478", "$4:3479"]
EOF
}

# run_case <topología> <direct|nodirect> [ipv6]
run_case() {
  local topo=$1 expect=$2 v6=${3:-no} url stun listen
  section "Topología: $topo (esperado: $expect)"
  topo_setup "$topo" || { bad "no se pudo construir $topo"; return; }
  rm -rf $LAB/a.state $LAB/b.state $LAB/*.sock
  if [ "$v6" = ipv6 ]; then url="http://[2001:db8:100::10]:8081"; stun="[2001:db8:100::10]"; listen="[::]:51820"
  else url="http://198.51.100.10:8081"; stun="198.51.100.10"; listen="0.0.0.0:51820"; fi
  issue a; issue b
  agentcfg a "$listen" "$url" "$stun"; agentcfg b "$listen" "$url" "$stun"

  ATENEA_COORD_LISTEN="[::]:8081" ATENEA_COORD_STUN_BIND="::" ATENEA_COORD_ANCHORS="[\"$ROOT_PUB\"]" \
    ip netns exec inet atenea-coordinator > $LAB/coord.log 2>&1 &
  sleep 1
  ( while true; do issue a; issue b; sleep $REFRESH; done ) &
  REFRESHER=$!
  ip netns exec a atenea up --config $LAB/a.toml --log $LAB/a.log >/dev/null
  ip netns exec b atenea up --config $LAB/b.toml --log $LAB/b.log >/dev/null

  local connected=no
  for _ in $(seq 1 $([ "$expect" = direct ] && echo 45 || echo 30)); do
    if ip netns exec a ping -c 1 -W 1 10.77.0.2 >/dev/null 2>&1; then connected=yes; break; fi
    sleep 1
  done
  local path_a path_b ep_a ep_b map_a map_b waited=0
  if [ "$expect" = direct ] && [ "$connected" = yes ]; then
    # El estado de ruta se confirma con el primer paquete autenticado de cada sentido: se espera, no se supone.
    for waited in $(seq 0 15); do
      [ "$(field a peers.0.path)" = direct ] && [ "$(field b peers.0.path)" = direct ] && break
      sleep 1
    done
    echo "       ambos extremos informan ruta directa tras ${waited} s adicionales al primer ping"
  fi
  path_a=$(field a peers.0.path); path_b=$(field b peers.0.path)
  ep_a=$(field a peers.0.endpoint); ep_b=$(field b peers.0.endpoint)
  map_a=$(field a nat.mapping); map_b=$(field b nat.mapping)
  echo "       NAT visto por STUN: a=$map_a  b=$map_b | ruta: a=$path_a ($ep_a)  b=$path_b ($ep_b)"
  echo "       reflexivo a=$(field a nat.reflexive)  b=$(field b nat.reflexive)"

  if [ "$expect" = direct ]; then
    [ "$connected" = yes ] && ok "ping A→B por el túnel" || bad "sin conectividad por el túnel"
    expect_eq "A informa ruta directa" "$path_a" direct
    expect_eq "B informa ruta directa" "$path_b" direct
    if [ "$connected" = yes ]; then
      ip netns exec b python3 "$TOOLS" tcp-server 10.77.0.2 9000 > $LAB/srv.json &
      local S=$!; sleep 0.5
      ip netns exec a python3 "$TOOLS" tcp-client 10.77.0.2 9000 2000000 9 > $LAB/cli.json
      wait $S
      expect_eq "TCP 2 MB íntegro por la ruta directa" "$(python3 -c "import json;print(json.load(open('$LAB/srv.json'))['sha256'])")" "$(python3 -c "import json;print(json.load(open('$LAB/cli.json'))['sha256'])")"
    fi
  else
    [ "$connected" = no ] && ok "sin P2P: el túnel no se establece (esperado)" || bad "se conectó, pero se esperaba que no"
    [ "$path_a" != direct ] && ok "A no declara ruta directa ($path_a)" || bad "A declara ruta directa"
    expect_eq "A detecta el mapeo dependiente del destino" "$map_a" dependent
    for _ in $(seq 1 20); do [ "$(field a counters.probe_failed)" != error ] && [ "$(field a counters.probe_failed)" -ge 1 ] 2>/dev/null && break; sleep 1; done
    [ "$(field a counters.probe_failed)" -ge 1 ] 2>/dev/null && ok "A registra probe_failed (a la espera del relay de la fase 5)" || bad "A no registra probe_failed"
  fi
}

# ---- casos ----
run_case flat direct
case_flat_ep_a=$(field a peers.0.endpoint)
[[ "$case_flat_ep_a" == 198.51.100.* ]] && ok "flat: ruta por la dirección pública directa" || bad "flat: endpoint inesperado $case_flat_ep_a"

run_case home direct
[[ "$(field a peers.0.endpoint)" == 198.51.100.* ]] && ok "home: ruta por el endpoint reflexivo (NAT de B)" || bad "home: la ruta no usa el endpoint reflexivo"

# Cambio de red: a pasa a otra LAN tras otro NAT; la ruta debe recuperarse
section "Cambio de red de A (home → café)"
EP_B_BEFORE=$(field b peers.0.endpoint)
T0=$(date +%s)
topo_move_a_to_cafe
RECOVERED=no
for _ in $(seq 1 150); do
  if ip netns exec a ping -c 1 -W 1 10.77.0.2 >/dev/null 2>&1 && [ "$(field b peers.0.endpoint)" != "$EP_B_BEFORE" ]; then RECOVERED=yes; break; fi
  sleep 1
done
T=$(( $(date +%s) - T0 ))
EP_B_AFTER=$(field b peers.0.endpoint)
echo "       endpoint de A visto por B: $EP_B_BEFORE → $EP_B_AFTER (recuperación en ${T}s)"
[ "$RECOVERED" = yes ] && ok "tras el cambio de red la conectividad se restablece con otro endpoint" || bad "no se recuperó en 150 s"
expect_eq "ruta directa de nuevo" "$(field a peers.0.path)" direct
kill $REFRESHER 2>/dev/null

run_case double direct
run_case symmetric nodirect
run_case samelan_hairpin_off direct
[[ "$(field a peers.0.endpoint)" == 192.168.1.* ]] && ok "misma LAN sin hairpin: ruta por la dirección local (sin pasar por el NAT)" || bad "misma LAN: endpoint inesperado $(field a peers.0.endpoint)"
run_case samelan_hairpin_on direct
run_case ipv6 direct ipv6
[[ "$(field a peers.0.endpoint)" == \[2001:db8:b::* ]] && ok "ipv6: ruta directa por IPv6 con cortafuegos con estado" || bad "ipv6: endpoint inesperado $(field a peers.0.endpoint)"

# Sin dispositivos fantasma: al parar los agentes la presencia caduca (TTL 60 s)
section "Presencia con TTL: sin dispositivos fantasma"
P0=$(ip netns exec inet python3 -c "import urllib.request,json;print(json.load(urllib.request.urlopen('http://[::1]:8081/healthz',timeout=3))['presence'])" 2>/dev/null || echo error)
echo "       presencias con los agentes activos: $P0"
atenea down --socket $LAB/a.sock; atenea down --socket $LAB/b.sock
kill $REFRESHER 2>/dev/null
for _ in $(seq 1 90); do
  P=$(ip netns exec inet python3 -c "import urllib.request,json;print(json.load(urllib.request.urlopen('http://[::1]:8081/healthz',timeout=3))['presence'])" 2>/dev/null || echo error)
  [ "$P" = 0 ] && break; sleep 1
done
[ "$P0" -ge 2 ] 2>/dev/null && ok "dos presencias activas mientras los agentes anuncian" || bad "presencias iniciales inesperadas ($P0)"
expect_eq "presencias tras parar los agentes (≤ TTL 60 s + barrido)" "$P" 0

echo
if [ $FAILS -eq 0 ]; then echo "E2E NAT: TODO OK"; else echo "E2E NAT: $FAILS fallo(s)"; fi
exit $FAILS
