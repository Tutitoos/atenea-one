#!/usr/bin/env bash
# Laboratorio de la fase 2: dos (y un tercer, no autorizado) agentes en namespaces sobre un puente.
# Se ejecuta DENTRO del contenedor (ver run.sh). Sale con código != 0 si falla algún caso.
set -uo pipefail

LAB=/tmp/lab
TOOLS=$PWD/tests/network-lab/tools.py
T() { python3 "$TOOLS" "$@"; }
FAILS=0
ok()   { echo "  OK   $*"; }
bad()  { echo "  FAIL $*"; FAILS=$((FAILS + 1)); }
check() { local d=$1; shift; if "$@" >/dev/null 2>&1; then ok "$d"; else bad "$d"; fi; }
expect_eq() { if [ -n "$2" ] && [ "$2" = "$3" ]; then ok "$1 ($2)"; else bad "$1: esperado '$3', obtenido '$2'"; fi; }
ctr() { T counter "$LAB/$1.sock" "$2"; }
section() { echo; echo "== $*"; }

section "Compilación y pruebas unitarias (Linux)"
cargo build --release --locked -q 2>&1 | tail -3 || exit 1
cargo test --workspace --locked -q 2>&1 | grep -E "test result|FAILED|error" | sort | uniq -c
BIN=$CARGO_TARGET_DIR/release
export PATH=$BIN:$PATH

section "Entorno"
rm -rf "$LAB"; mkdir -p "$LAB"
ip link add br0 type bridge && ip link set br0 up
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
for n in a b c; do atenea keygen --out $LAB/$n.key > $LAB/$n.pub; done
PUB_A=$(cat $LAB/a.pub); PUB_B=$(cat $LAB/b.pub); PUB_C=$(cat $LAB/c.pub)
NET=00112233445566778899aabbccddeeff

# cfg <ns> <idx> [peer_pub peer_idx peer_endpoint]...
cfg() {
  local n=$1 i=$2; shift 2
  {
    echo "network_id = \"$NET\""
    echo "private_key_file = \"$LAB/$n.key\""
    echo "interface = \"atenea0\""
    echo "listen = \"0.0.0.0:51820\""
    echo "ipc_socket = \"$LAB/$n.sock\""
    echo "addresses = [\"10.77.0.$i\", \"fd77::$i\"]"
    echo "networks = [\"10.77.0.0/24\", \"fd77::/64\"]"
    while [ $# -gt 0 ]; do
      echo "[[peer]]"; echo "public_key = \"$1\""
      echo "addresses = [\"10.77.0.$2\", \"fd77::$2\"]"
      [ -n "$3" ] && echo "endpoint = \"$3:51820\""
      shift 3
    done
  } > $LAB/$n.toml
}
cfg a 1 "$PUB_B" 2 192.0.2.2
cfg b 2 "$PUB_A" 1 192.0.2.1
cfg c 3 "$PUB_B" 2 192.0.2.2   # C no está autorizado en B

up()   { ip netns exec $1 atenea up --config $LAB/$1.toml --log $LAB/$1.log; }
down() { atenea down --socket $LAB/$1.sock; sleep 0.5; }

tcpdump -i br0 -U -w $LAB/cap.pcap udp >/dev/null 2>&1 &
TCPDUMP=$!
sleep 1
up a && up b && ok "agentes A y B arrancados con 'atenea up'" || { bad "arranque"; cat $LAB/a.log $LAB/b.log 2>/dev/null; exit 1; }

section "Conectividad IPv4 e IPv6"
check "ping IPv4 A→B (dispara el handshake)" ip netns exec a ping -c 3 -W 3 10.77.0.2
check "ping IPv6 A→B" ip netns exec a ping -6 -c 3 -W 3 fd77::2
check "ping IPv4 B→A" ip netns exec b ping -c 3 -W 3 10.77.0.1
check "ping IPv6 B→A" ip netns exec b ping -6 -c 3 -W 3 fd77::1

section "Transferencia TCP/UDP íntegra (IPv4 e IPv6)"
for fam in "10.77.0.2 4" "fd77::2 6"; do
  set -- $fam; dst=$1; v=$2
  ip netns exec b python3 "$TOOLS" tcp-server $dst 9000 > $LAB/srv$v.json &
  S=$!
  ip netns exec a python3 "$TOOLS" tcp-client $dst 9000 5000000 $v > $LAB/cli$v.json
  wait $S
  expect_eq "TCP IPv$v: 5 MB, sha256 coincide" "$(python3 -c "import json;print(json.load(open('$LAB/srv$v.json'))['sha256'])")" "$(python3 -c "import json;print(json.load(open('$LAB/cli$v.json'))['sha256'])")"
  echo "       (rendimiento orientativo IPv$v: $(python3 -c "import json;print(json.load(open('$LAB/cli$v.json'))['mbit_s'])") Mbit/s)"
  ip netns exec b python3 "$TOOLS" udp-server $dst 9001 3 > $LAB/udp$v.txt &
  S=$!; sleep 0.5
  ip netns exec a python3 "$TOOLS" udp-client $dst 9001 300
  wait $S
  expect_eq "UDP IPv$v: 300/300 datagramas" "$(cat $LAB/udp$v.txt)" 300
done

section "MTU documentada (1400)"
check "IPv4: 1400 bytes con DF pasa"   ip netns exec a ping -c 1 -W 3 -M do -s 1372 10.77.0.2
check "IPv4: 1401 bytes con DF se rechaza" bash -c '! ip netns exec a ping -c 1 -W 3 -M do -s 1373 10.77.0.2'
check "IPv6: 1400 bytes pasa"          ip netns exec a ping -6 -c 1 -W 3 -M do -s 1352 fd77::2
check "IPv6: 1401 bytes se rechaza"    bash -c '! ip netns exec a ping -6 -c 1 -W 3 -M do -s 1353 fd77::2'

section "Sin payload en claro en la captura externa"
MARK=ATENEA-SECRET-MARKER-9f3c1d
ip netns exec b python3 "$TOOLS" tcp-server 10.77.0.2 9002 > $LAB/mark.json &
S=$!; sleep 0.5
ip netns exec a python3 -c "
import socket
s=socket.create_connection(('10.77.0.2',9002));s.sendall(('$MARK'*100).encode());s.close()"
wait $S
expect_eq "el marcador llegó completo a B por el túnel (bytes)" "$(python3 -c "import json;print(json.load(open('$LAB/mark.json'))['bytes'])")" "$(( ${#MARK} * 100 ))"
# control: el mismo texto fuera del túnel SÍ debe verse en la captura (valida el método)
ip netns exec a python3 "$TOOLS" send-udp 192.0.2.2 9999 "$MARK-CONTROL"
sleep 1
kill -INT $TCPDUMP; wait $TCPDUMP 2>/dev/null
expect_eq "control en claro visible en la captura (1 aparición)" "$(T contains $LAB/cap.pcap $MARK-CONTROL)" 1
# el control contiene $MARK una vez; cualquier aparición adicional sería payload del túnel en claro
expect_eq "total de apariciones del marcador = solo el control (túnel sin payload en claro)" "$(T contains $LAB/cap.pcap $MARK)" 1

section "Rechazos de datagramas hostiles (contadores del agente B)"
tcpdump -i br0 -U -w $LAB/cap2.pcap udp >/dev/null 2>&1 &
TCPDUMP=$!; sleep 1
ip netns exec a ping -c 4 -W 3 10.77.0.2 >/dev/null
sleep 1; kill -INT $TCPDUMP; wait $TCPDUMP 2>/dev/null
DG=$(T data-datagram $LAB/cap2.pcap 192.0.2.1 51820 2)
EP_BEFORE=$(ctr b endpoint_updates)
R0=$(ctr b rx_replay); A0=$(ctr b rx_auth_fail); O0=$(ctr b rx_oversize)
ip netns exec c python3 "$TOOLS" inject 192.0.2.2 51820 "$DG" replay
ip netns exec c python3 "$TOOLS" inject 192.0.2.2 51820 "$DG" bump-counter
ip netns exec c python3 "$TOOLS" inject 192.0.2.2 51820 "$DG" oversize
sleep 1
expect_eq "replay rechazado" "$(( $(ctr b rx_replay) - R0 ))" 1
expect_eq "contador alterado: fallo de autenticación" "$(( $(ctr b rx_auth_fail) - A0 ))" 1
expect_eq "datagrama sobredimensionado descartado" "$(( $(ctr b rx_oversize) - O0 ))" 1
expect_eq "el endpoint de A no cambió por paquetes no autenticados" "$(ctr b endpoint_updates)" "$EP_BEFORE"
check "el túnel sigue funcionando tras el abuso" ip netns exec a ping -c 2 -W 3 10.77.0.2

section "Peer no autorizado y clave equivocada"
up c
U0=$(ctr b rx_init_unknown_peer)
check "C (no autorizado) no alcanza a B" bash -c '! ip netns exec c ping -c 2 -W 2 10.77.0.2'
sleep 1
[ "$(( $(ctr b rx_init_unknown_peer) - U0 ))" -ge 1 ] && ok "B descartó el handshake de C sin responder (rx_init_unknown_peer)" || bad "B no registró a C"
down c

section "Pérdida, reordenación y latencia (netem 8% pérdida, 20 ms ±10, reorder 25%)"
for n in a b; do tc qdisc add dev va-$n root netem delay 20ms 10ms loss 8% reorder 25% 50% 2>/dev/null || tc qdisc add dev va-$n root netem delay 20ms 10ms loss 8%; done
ip netns exec b python3 "$TOOLS" tcp-server 10.77.0.2 9003 > $LAB/srvl.json &
S=$!
ip netns exec a python3 "$TOOLS" tcp-client 10.77.0.2 9003 2000000 3 > $LAB/clil.json
wait $S
expect_eq "TCP 2 MB íntegro con pérdida/reorden" "$(python3 -c "import json;print(json.load(open('$LAB/srvl.json'))['sha256'])")" "$(python3 -c "import json;print(json.load(open('$LAB/clil.json'))['sha256'])")"
ip netns exec b python3 "$TOOLS" udp-server 10.77.0.2 9004 4 > $LAB/udpl.txt &
S=$!; sleep 0.5
ip netns exec a python3 "$TOOLS" udp-client 10.77.0.2 9004 500
wait $S
echo "       UDP con pérdida: $(cat $LAB/udpl.txt)/500 recibidos (esperado ≈ 92 %, sin duplicados contados dos veces)"
[ "$(cat $LAB/udpl.txt)" -gt 350 ] && [ "$(cat $LAB/udpl.txt)" -lt 500 ] && ok "UDP: pérdida de la red visible, el resto llega" || bad "UDP con pérdida fuera de rango"
echo "       rx_replay B durante la prueba: $(ctr b rx_replay) (duplicados/reordenación de netem rechazados o aceptados según ventana)"
for n in a b; do tc qdisc del dev va-$n root; done

section "Reinicio del agente A: sesión nueva, sin reutilizar nonces"
HS0=$(ctr b handshakes_responder)
down a
up a
check "ping tras reiniciar A" ip netns exec a ping -c 3 -W 3 10.77.0.2
sleep 1
expect_eq "B negoció una sesión nueva (handshake adicional)" "$(( $(ctr b handshakes_responder) - HS0 ))" 1

section "Parada limpia: no queda interfaz ni rutas"
down a; down b
check "interfaz atenea0 eliminada en A" bash -c '! ip -n a link show atenea0'
check "sin rutas 10.77/fd77 residuales en A" bash -c '[ -z "$(ip -n a route; ip -n a -6 route)" ] || ! (ip -n a route; ip -n a -6 route) | grep -qE "10.77|fd77"'
check "socket IPC eliminado" bash -c "[ ! -e $LAB/a.sock ]"

kill $TCPDUMP 2>/dev/null
echo
if [ $FAILS -eq 0 ]; then echo "LABORATORIO: TODO OK"; else echo "LABORATORIO: $FAILS fallo(s)"; fi
exit $FAILS
