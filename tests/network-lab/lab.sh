#!/usr/bin/env bash
# Copyright 2026 Tutitoos
# SPDX-License-Identifier: Apache-2.0
# Laboratorio (fases 2 y 3): dos agentes autorizados (A y B) y un tercero no autorizado (C) en namespaces
# sobre un puente, con configuración firmada emitida por un "emisor" que es un bucle de reemisión.
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
ctr()   { T counter "$LAB/$1.sock" "$2"; }
field() { T field "$LAB/$1.sock" "$2"; }
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

NET=00112233445566778899aabbccddeeff
VALID=40       # validez de laboratorio de cada configuración (s); producción propuesta: 300
REFRESH=10     # cadencia de reemisión de laboratorio (s); producción propuesta: 60

# Jerarquía de confianza (ADR-005): raíz → emisor → configuraciones
atenea root-init --out $LAB/root.key > $LAB/root.pub
ROOT_PUB=$(cat $LAB/root.pub)
atenea issuer-init --root $LAB/root.key --network-id $NET --serial 1 --valid-days 30 --out-dir $LAB/issuer > /dev/null
for n in a b c; do atenea device-init --dir $LAB/$n > $LAB/$n.info; done
devid() { sed -n 's/^device_id=//p' $LAB/$1.info; }

# spec <ns> <idx> [peer_ns peer_idx peer_endpoint]...
spec() {
  local n=$1 i=$2; shift 2
  local out=$LAB/$n.spec.toml
  {
    echo "network_id = \"$NET\""
    echo "device_bundle = \"$LAB/$n/bundle.bin\""
    echo "validity_secs = $VALID"
    echo "tunnel_addrs = [\"10.77.0.$i\", \"fd77::$i\"]"
    echo "network_prefixes = [\"10.77.0.0/24\", \"fd77::/64\"]"
    local has_peer=0
    while [ $# -gt 0 ]; do
      has_peer=1
      echo "[[peer]]"
      echo "bundle = \"$LAB/$1/bundle.bin\""
      echo "addrs = [\"10.77.0.$2\", \"fd77::$2\"]"
      [ -n "$3" ] && echo "endpoints = [\"$3:51820\"]"
      shift 3
    done
    echo "[[rule]]"; echo 'src = "any"'; echo 'dst = "any"'; echo 'proto = "icmp"'
    echo "[[rule]]"; echo 'src = "any"'; echo 'dst = "any"'; echo 'proto = "icmpv6"'
    if [ $has_peer = 1 ]; then
      for proto in tcp udp; do
        if [ "$n" = b ]; then
          echo "[[rule]]"; echo 'src = "peer:0"'; echo 'dst = "self"'; echo "proto = \"$proto\""; echo 'ports = "9000-9100"'
          echo "[[rule]]"; echo 'src = "self"'; echo 'dst = "peer:0"'; echo "proto = \"$proto\""
        else
          echo "[[rule]]"; echo 'src = "self"'; echo 'dst = "peer:0"'; echo "proto = \"$proto\""; echo 'ports = "9000-9100"'
          echo "[[rule]]"; echo 'src = "peer:0"'; echo 'dst = "self"'; echo "proto = \"$proto\""
        fi
      done
    fi
  } > $out
}
issue() { atenea issue --issuer-key $LAB/issuer/issuer.key --issuer-cert $LAB/issuer/issuer.cert --spec $LAB/$1.spec.toml --out $LAB/$1.cfg 2>/dev/null; }

spec a 1 b 2 192.0.2.2
spec b 2 a 1 192.0.2.1
spec c 3 b 2 192.0.2.2     # C está firmado por el emisor pero B no lo conoce
issue a; issue b; issue c

# agentcfg <ns> <idx>
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
EOF
}
agentcfg a; agentcfg b; agentcfg c

up()   { ip netns exec $1 atenea up --config $LAB/$1.toml --log $LAB/$1.log; }
down() { atenea down --socket $LAB/$1.sock; sleep 0.5; }

# "Emisor": reemite A y B cada $REFRESH s mientras exista el fichero de control
touch $LAB/issuing
( while [ -e $LAB/issuing ]; do issue a; issue b; sleep $REFRESH; done ) &
ISSUER=$!

tcpdump -i br0 -U -w $LAB/cap.pcap udp >/dev/null 2>&1 &
TCPDUMP=$!
sleep 1
up a && up b && ok "agentes A y B arrancados con 'atenea up' y configuración firmada" || { bad "arranque"; cat $LAB/a.log $LAB/b.log 2>/dev/null; exit 1; }
expect_eq "A: lease activo tras cargar la configuración" "$(field a config.lease_active)" true
check "estado persistente con permisos 0600" bash -c "[ \"\$(stat -c %a $LAB/a.state/state.bin)\" = 600 ]"

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

section "ACL: denegación por defecto"
D0=$(ctr a tx_acl_denied)
ip netns exec a python3 "$TOOLS" send-udp 10.77.0.2 9500 "puerto no permitido"
ip netns exec a python3 "$TOOLS" send-udp 10.77.0.2 9500 "otra vez"
sleep 1
expect_eq "UDP a 9500 (fuera de 9000-9100) descartado en origen" "$(( $(ctr a tx_acl_denied) - D0 ))" 2
check "TCP a 9500 no conecta" bash -c "! ip netns exec a timeout 4 python3 $TOOLS tcp-client 10.77.0.2 9500 10 1"
R0=$(ctr b rx_acl_denied)
ip netns exec a python3 "$TOOLS" udp-client 10.77.0.2 9050 3
ip netns exec b python3 "$TOOLS" send-udp 10.77.0.1 9050 "respuesta" # B→A permitido por regla de respuestas
check "ICMP sigue permitido" ip netns exec a ping -c 1 -W 3 10.77.0.2
echo "       (rx_acl_denied en B: $(( $(ctr b rx_acl_denied) - R0 )); el ACL de entrada se comprueba en pruebas unitarias)"

section "Suplantación de IP de origen dentro del túnel"
S0=$(ctr a tx_bad_source)
ip netns exec a python3 "$TOOLS" spoof 10.77.0.9 10.77.0.2 9000
sleep 1
expect_eq "paquete con origen 10.77.0.9 descartado por A" "$(( $(ctr a tx_bad_source) - S0 ))" 1

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

section "Peer no autorizado (firmado por el emisor, pero ausente de la configuración de B)"
up c
U0=$(ctr b rx_init_unknown_peer)
check "C no alcanza a B" bash -c '! ip netns exec c ping -c 2 -W 2 10.77.0.2'
sleep 1
[ "$(( $(ctr b rx_init_unknown_peer) - U0 ))" -ge 1 ] && ok "B descartó el handshake de C sin responder (rx_init_unknown_peer)" || bad "B no registró a C"
down c

section "Configuraciones firmadas inválidas rechazadas (agente B), sin cortar el servicio vigente"
kill $ISSUER 2>/dev/null; wait $ISSUER 2>/dev/null   # congela al emisor mientras se inyectan ficheros
issue b; GOOD_B=$LAB/b.cfg.good; cp $LAB/b.cfg $GOOD_B
sleep 2
C0=$(ctr b config_rejected)
expect_eq_err() { expect_eq "$1" "$(field b config.last_error)" "$2"; }
# 1. firma alterada
cp $GOOD_B $LAB/b.cfg.t; T flip-last $LAB/b.cfg.t; mv $LAB/b.cfg.t $LAB/b.cfg; sleep 3
expect_eq_err "firma alterada → bad_config_signature" bad_config_signature
# 2. otro emisor/otra raíz
atenea root-init --out $LAB/root2.key >/dev/null
atenea issuer-init --root $LAB/root2.key --network-id $NET --serial 1 --valid-days 30 --out-dir $LAB/issuer2 >/dev/null
atenea issue --issuer-key $LAB/issuer2/issuer.key --issuer-cert $LAB/issuer2/issuer.cert --spec $LAB/b.spec.toml --out $LAB/b.cfg.t 2>/dev/null && mv $LAB/b.cfg.t $LAB/b.cfg; sleep 3
expect_eq_err "emisor de otra raíz → unknown_root" unknown_root
# 3. caducada
sed "s/^validity_secs = .*/validity_secs = 100\nnot_before = $(( $(date +%s) - 1000 ))/" $LAB/b.spec.toml > $LAB/b.old.toml
atenea issue --issuer-key $LAB/issuer/issuer.key --issuer-cert $LAB/issuer/issuer.cert --spec $LAB/b.old.toml --out $LAB/b.cfg.t 2>/dev/null && mv $LAB/b.cfg.t $LAB/b.cfg; sleep 3
expect_eq_err "configuración caducada → expired" expired
# 4. versión anterior (rollback): una versión vieja pero aún en ventana
sed "s/^validity_secs = .*/validity_secs = 100\nconfig_version = 1/" $LAB/b.spec.toml > $LAB/b.old.toml
atenea issue --issuer-key $LAB/issuer/issuer.key --issuer-cert $LAB/issuer/issuer.cert --spec $LAB/b.old.toml --out $LAB/b.cfg.t 2>/dev/null && mv $LAB/b.cfg.t $LAB/b.cfg; sleep 3
expect_eq_err "versión anterior (rollback) → config_rollback" config_rollback
# 5. basura
head -c 300 /dev/urandom > $LAB/b.cfg.t; mv $LAB/b.cfg.t $LAB/b.cfg; sleep 3
LAST=$(field b config.last_error)
case "$LAST" in too_large|malformed|unsupported_version) ok "fichero basura rechazado ($LAST)";; *) bad "fichero basura: error inesperado '$LAST'";; esac
expect_eq "5 rechazos contados" "$(( $(ctr b config_rejected) - C0 ))" 5
check "el servicio con la configuración vigente no se interrumpió" ip netns exec a ping -c 2 -W 3 10.77.0.2
# restauración: una emisión legítima nueva limpia el error
issue b; sleep 3
expect_eq "una emisión legítima posterior se aplica y limpia el error" "$(field b config.last_error)" none

section "Pérdida, reordenación y latencia (netem 8% pérdida, 20 ms ±10, reorder 25%)"
( while [ -e $LAB/issuing ]; do issue a; issue b; sleep $REFRESH; done ) &
ISSUER=$!
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
echo "       UDP con pérdida: $(cat $LAB/udpl.txt)/500 recibidos"
[ "$(cat $LAB/udpl.txt)" -gt 350 ] && [ "$(cat $LAB/udpl.txt)" -lt 500 ] && ok "UDP: pérdida de la red visible, el resto llega" || bad "UDP con pérdida fuera de rango"
for n in a b; do tc qdisc del dev va-$n root; done

section "Reinicio del agente A: sesión nueva y estado anti-rollback persistente"
V0=$(field a config.version)
HS0=$(ctr b handshakes_responder)
down a
up a
check "ping tras reiniciar A" ip netns exec a ping -c 3 -W 3 10.77.0.2
sleep 1
expect_eq "B negoció una sesión nueva (handshake adicional)" "$(( $(ctr b handshakes_responder) - HS0 ))" 1
[ "$(field a config.version)" -ge "$V0" ] && ok "A conserva una versión de configuración ≥ la anterior tras reiniciar" || bad "versión retrocedió"

section "Revocación por lease (ADR-002; validez $VALID s, tolerancia 30 s, reemisión cada $REFRESH s)"
# a) Peers: el emisor deja de listar a A en la configuración de B → B corta a A en la siguiente recarga
kill $ISSUER 2>/dev/null; wait $ISSUER 2>/dev/null   # sin carreras con el bucle de reemisión
cp $LAB/b.spec.toml $LAB/b.spec.full
spec b 2        # B sin peers
T0=$(date +%s); issue b
FAILS_IN_ROW=0
while [ $(( $(date +%s) - T0 )) -lt 15 ]; do
  if ip netns exec a ping -c 1 -W 1 10.77.0.2 >/dev/null 2>&1; then FAILS_IN_ROW=0; else FAILS_IN_ROW=$((FAILS_IN_ROW + 1)); fi
  [ $FAILS_IN_ROW -ge 2 ] && break
  sleep 0.3
done
CUT=$(( $(date +%s) - T0 ))
[ $FAILS_IN_ROW -ge 2 ] && [ $CUT -le 8 ] && ok "B corta a A en ${CUT}s tras reemitir su configuración sin A (recarga ≤ 1 s + sondeo)" || bad "B no cortó a A a tiempo (${CUT}s)"
check "A sigue sin alcanzar a B" bash -c '! ip netns exec a ping -c 2 -W 1 10.77.0.2'
# b) El propio A: el emisor deja de renovarlo; debe cortarse solo al caducar su lease + tolerancia
kill $ISSUER 2>/dev/null; wait $ISSUER 2>/dev/null
cp $LAB/b.spec.full $LAB/b.spec.toml; issue b      # B recupera a A mientras A no se renueva
T1=$(date +%s)                                      # última configuración de A emitida hace ≤ $REFRESH s: cota conservadora
issue a; T1=$(date +%s)
sleep $(( VALID - 5 ))
expect_eq "A sigue activo antes de caducar su lease (t≈$(( VALID - 5 )) s)" "$(field a config.lease_active)" true
for _ in $(seq 1 100); do [ "$(field a config.lease_active)" = false ] && break; sleep 1; done
CUT=$(( $(date +%s) - T1 ))
echo "       A se cortó a sí mismo ${CUT}s después de su última configuración (esperado entre $VALID y $(( VALID + 30 + 2 )))"
[ $CUT -ge $VALID ] && [ $CUT -le $(( VALID + 32 )) ] && ok "corte de A dentro de validez + tolerancia" || bad "corte de A fuera del rango"
D0=$(ctr a denied_no_lease)
ip netns exec a ping -c 2 -W 1 10.77.0.2 >/dev/null 2>&1
[ "$(( $(ctr a denied_no_lease) - D0 ))" -ge 1 ] && ok "A deniega el tráfico (fail-closed, denied_no_lease)" || bad "A no deniega con el lease caducado"
# c) Recuperación: una configuración vigente restablece el servicio
issue a; issue b
( while [ -e $LAB/issuing ]; do issue a; issue b; sleep $REFRESH; done ) &
ISSUER=$!
for _ in $(seq 1 20); do ip netns exec a ping -c 1 -W 1 10.77.0.2 >/dev/null 2>&1 && break; sleep 1; done
check "tras reemitir, A y B vuelven a comunicarse" ip netns exec a ping -c 2 -W 2 10.77.0.2

section "Parada limpia: no queda interfaz ni rutas"
down a; down b
check "interfaz atenea0 eliminada en A" bash -c '! ip -n a link show atenea0'
check "sin rutas 10.77/fd77 residuales en A" bash -c '[ -z "$(ip -n a route; ip -n a -6 route)" ] || ! (ip -n a route; ip -n a -6 route) | grep -qE "10.77|fd77"'
check "socket IPC eliminado" bash -c "[ ! -e $LAB/a.sock ]"

rm -f $LAB/issuing; kill $ISSUER 2>/dev/null; kill $TCPDUMP 2>/dev/null
echo
if [ $FAILS -eq 0 ]; then echo "LABORATORIO: TODO OK"; else echo "LABORATORIO: $FAILS fallo(s)"; fi
exit $FAILS
