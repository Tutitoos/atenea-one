#!/usr/bin/env bash
# Copyright 2026 Tutitoos
# SPDX-License-Identifier: Apache-2.0
# Laboratorio de la fase 7 (dentro del contenedor, ver run.sh): subred servida por un router (r) a un cliente (a),
# con una LAN interna emulada (hosts h y h2), un segundo cliente (a2) con una LAN local solapada, control local
# por sockets, SNAT y restauración. DNS: `resolvectl` simulado (la prueba real con systemd-resolved está en
# tests/packaging). No hay Proxmox: la LAN son espacios de nombres.
set -uo pipefail

LAB=/tmp/lab
TOOLS=$PWD/tests/network-lab/tools.py
T() { python3 "$TOOLS" "$@"; }
FAILS=0
ok()   { echo "  OK   $*"; }
bad()  { echo "  FAIL $*"; FAILS=$((FAILS + 1)); }
check() { local d=$1; shift; if "$@" >/dev/null 2>&1; then ok "$d"; else bad "$d"; fi; }
expect_eq() { if [ -n "$2" ] && [ "$2" = "$3" ]; then ok "$1 ($2)"; else bad "$1: esperado '$3', obtenido '$2'"; fi; }
field() { T field "$LAB/$1.sock" "$2" 2>/dev/null || echo error; }
ctr()   { T counter "$LAB/$1.sock" "$2" 2>/dev/null || echo 0; }
section() { echo; echo "== $*"; }
wait_for() { local n=$1; shift; for _ in $(seq 1 "$n"); do "$@" >/dev/null 2>&1 && return 0; sleep 1; done; return 1; }

section "Compilación"
cargo build --release --locked -q 2>&1 | tail -3
export PATH=$CARGO_TARGET_DIR/release:$PATH
rm -rf "$LAB"; mkdir -p "$LAB/bin"

# resolvectl simulado: registra cada llamada y acepta todo (systemd-resolved real: tests/packaging)
cat > $LAB/bin/resolvectl <<'EOF'
#!/bin/sh
echo "$@" >> /tmp/lab/resolvectl.log
exit 0
EOF
chmod +x $LAB/bin/resolvectl
export ATENEA_RESOLVECTL=$LAB/bin/resolvectl PATH=$LAB/bin:$PATH

section "Entorno: a, a2 (clientes), r (router de subred), h/h2 (VMs de la LAN), h3 (LAN local de a2)"
ip link add br0 type bridge && ip link set br0 up
ip link add brlan type bridge && ip link set brlan up
ip link add brlan3 type bridge && ip link set brlan3 up
for spec in a:1 r:2 a2:3; do
  n=${spec%%:*}; i=${spec##*:}
  ip netns add $n; ip link add va-$n type veth peer name v$n; ip link set v$n netns $n
  ip link set va-$n master br0 up
  ip -n $n link set lo up; ip -n $n addr add 192.0.2.$i/24 dev v$n; ip -n $n link set v$n up
done
for n in h h2 h3; do ip netns add $n; ip -n $n link set lo up; done
attach() { # attach <ns> <ifname> <bridge>
  ip link add "vl-$1" type veth peer name "tmp-$1"; ip link set "tmp-$1" netns "$1"
  ip -n "$1" link set "tmp-$1" name "$2"; ip link set "vl-$1" master "$3" up; ip -n "$1" link set "$2" up
}
attach r lan0 brlan
ip -n r addr add 192.168.50.1/24 dev lan0; ip -n r -6 addr add fd50::1/64 dev lan0 nodad
attach h eth0 brlan
ip -n h addr add 192.168.50.20/24 dev eth0; ip -n h -6 addr add fd50::20/64 dev eth0 nodad
ip -n h route add default via 192.168.50.1; ip -n h -6 route add default via fd50::1
attach h2 eth0 brlan   # sin ruta de retorno hacia el túnel (su puerta de enlace no existe)
ip -n h2 addr add 192.168.50.30/24 dev eth0; ip -n h2 route add default via 192.168.50.254
attach a2 lan0 brlan3  # LAN local de a2 con el MISMO prefijo que la subred remota
ip -n a2 addr add 192.168.50.1/24 dev lan0
attach h3 eth0 brlan3
ip -n h3 addr add 192.168.50.99/24 dev eth0; ip -n h3 route add default via 192.168.50.1

NET=00112233445566778899aabbccddeeff
VALID=40; REFRESH=10
atenea root-init --out $LAB/root.key > $LAB/root.pub
ROOT_PUB=$(cat $LAB/root.pub)
atenea issuer-init --root $LAB/root.key --network-id $NET --serial 1 --valid-days 30 --out-dir $LAB/issuer > /dev/null
for n in a r a2; do atenea device-init --dir $LAB/$n > $LAB/$n.info; done

spec_client() { # spec_client <ns> <idx> [1 = con rutas y DNS | 0 = sin ellos]
  local n=$1 i=$2
  {
    echo "network_id = \"$NET\""; echo "device_bundle = \"$LAB/$n/bundle.bin\""; echo "validity_secs = $VALID"
    echo "tunnel_addrs = [\"10.77.0.$i\", \"fd77::$i\"]"
    echo 'network_prefixes = ["10.77.0.0/24", "fd77::/64"]'
    echo "[[peer]]"; echo "bundle = \"$LAB/r/bundle.bin\""; echo 'addrs = ["10.77.0.2", "fd77::2"]'
    echo 'endpoints = ["192.0.2.2:51820"]'
    [ "${3:-1}" = 1 ] && echo 'routes = ["192.168.50.0/24", "fd50::/64"]'
    for p in icmp icmpv6; do echo "[[rule]]"; echo 'src = "any"'; echo 'dst = "any"'; echo "proto = \"$p\""; done
    for d in $([ "${3:-1}" = 1 ] && echo "192.168.50.0/24 fd50::/64"); do
      echo "[[rule]]"; echo 'src = "self"'; echo "dst = \"prefix:$d\""; echo 'proto = "tcp"'; echo 'ports = "9000-9100"'
      echo "[[rule]]"; echo "src = \"prefix:$d\""; echo 'dst = "self"'; echo 'proto = "tcp"'
    done
    if [ "${3:-1}" = 1 ]; then
      echo "[dns]"; echo 'servers = ["10.77.0.2"]'; echo 'domains = ["lab.internal"]'
    fi
  } > $LAB/$n.spec.toml
}
spec_router() { # spec_router <snat: true|false>
  local snat=$1
  {
    echo "network_id = \"$NET\""; echo "device_bundle = \"$LAB/r/bundle.bin\""; echo "validity_secs = $VALID"
    echo 'tunnel_addrs = ["10.77.0.2", "fd77::2"]'
    echo 'network_prefixes = ["10.77.0.0/24", "fd77::/64"]'
    echo "[[peer]]"; echo "bundle = \"$LAB/a/bundle.bin\""; echo 'addrs = ["10.77.0.1", "fd77::1"]'
    echo "[[peer]]"; echo "bundle = \"$LAB/a2/bundle.bin\""; echo 'addrs = ["10.77.0.3", "fd77::3"]'
    echo "[[advertise]]"; echo 'prefix = "192.168.50.0/24"'; echo "snat = $snat"
    echo "[[advertise]]"; echo 'prefix = "fd50::/64"'; echo "snat = $snat"
    for p in icmp icmpv6; do echo "[[rule]]"; echo 'src = "any"'; echo 'dst = "any"'; echo "proto = \"$p\""; done
    for d in 192.168.50.0/24 fd50::/64; do
      for peer in 0 1; do
        echo "[[rule]]"; echo "src = \"peer:$peer\""; echo "dst = \"prefix:$d\""; echo 'proto = "tcp"'; echo 'ports = "9000-9100"'
        echo "[[rule]]"; echo "src = \"prefix:$d\""; echo "dst = \"peer:$peer\""; echo 'proto = "tcp"'
      done
    done
    echo "[dns]"; echo 'servers = ["10.77.0.2"]'; echo 'domains = ["lab.internal"]'
  } > $LAB/r.spec.toml
}
issue() { atenea issue --issuer-key $LAB/issuer/issuer.key --issuer-cert $LAB/issuer/issuer.cert --spec $LAB/$1.spec.toml --out $LAB/$1.cfg 2>>$LAB/issue.err; }
reissue_all() { issue a; issue r; issue a2; }
spec_client a 1; spec_client a2 3; spec_router false
reissue_all || { cat $LAB/issue.err; exit 1; }

agentcfg() { # agentcfg <ns> [extra]
  cat > $LAB/$1.toml <<EOF
interface = "atenea0"
listen = "0.0.0.0:51820"
ipc_socket = "$LAB/$1.sock"
noise_key_file = "$LAB/$1/noise.key"
identity_key_file = "$LAB/$1/identity.key"
config_file = "$LAB/$1.cfg"
state_dir = "$LAB/$1.state"
network_id = "$NET"
device_id = "$(sed -n 's/^device_id=//p' $LAB/$1.info)"
trust_anchor = "$ROOT_PUB"
${2:-}
EOF
}
agentcfg a; agentcfg a2
agentcfg r "status_socket = \"$LAB/r.status.sock\"
status_gid = 4242"
up()   { ip netns exec $1 atenea up --config $LAB/$1.toml --log $LAB/$1.log; }
down() { atenea down --socket $LAB/$1.sock >/dev/null 2>&1; sleep 0.5; }

touch $LAB/issuing
( while [ -e $LAB/issuing ]; do reissue_all; sleep $REFRESH; done ) &
ISSUER=$!
tcpdump -i br0 -U -w $LAB/cap.pcap udp >/dev/null 2>&1 &
TCPDUMP=$!
sleep 1
# Ruta por defecto de referencia (inaccesible a propósito): debe seguir idéntica tras instalar las rutas
ip -n a route add default via 192.0.2.254 dev va0 2>/dev/null || ip -n a route add default via 192.0.2.254
ip netns exec r sysctl -qw net.ipv4.ip_forward=0 net.ipv6.conf.all.forwarding=0
DEF_A=$(ip -n a route show default; ip -n a -6 route show default)
up r && up a && up a2 && ok "agentes r, a y a2 arrancados" || { bad "arranque"; cat $LAB/*.log; exit 1; }
wait_for 20 test "$(field a config.lease_active)" = true
wait_for 20 test "$(field r config.lease_active)" = true

section "Rutas del cliente: instaladas, sin tocar la LAN ni la ruta por defecto"
check "a tiene 192.168.50.0/24 por atenea0" bash -c "ip -n a route show 192.168.50.0/24 | grep -q 'dev atenea0'"
check "a tiene fd50::/64 por atenea0" bash -c "ip -n a -6 route show fd50::/64 | grep -q 'dev atenea0'"
expect_eq "estado de la ruta IPv4 en a" "$(T field $LAB/a.sock routes.0.state 2>/dev/null)" installed
expect_eq "la ruta por defecto de a no cambió" "$(ip -n a route show default; ip -n a -6 route show default)" "$DEF_A"
check "DNS dividido: solo el dominio ~lab.internal por el enlace" bash -c "grep -q 'dns atenea0 10.77.0.2' $LAB/resolvectl.log && grep -q 'domain atenea0 ~lab.internal' $LAB/resolvectl.log"
check "DNS: nunca '~.' (no captura el DNS general)" bash -c "! grep -qE '~\\.( |\$)|~\\.\$' $LAB/resolvectl.log"
expect_eq "dns informado por a" "$(field a dns)" applied

section "Forwarding: el agente informa, no cambia sysctls"
expect_eq "r informa forwarding_ok=false mientras el administrador no lo activa" "$(field r forwarding_ok)" false
check "el agente no activó ip_forward por su cuenta" bash -c "[ \"\$(ip netns exec r cat /proc/sys/net/ipv4/ip_forward)\" = 0 ]"
check "sin forwarding el cliente no alcanza la VM" bash -c '! ip netns exec a ping -c 2 -W 1 192.168.50.20'
ip netns exec r sysctl -qw net.ipv4.ip_forward=1 net.ipv6.conf.all.forwarding=1
sleep 6
expect_eq "r informa forwarding_ok=true tras activarlo el administrador" "$(field r forwarding_ok)" true

section "Acceso real a la VM interna (sin SNAT: ruta de retorno por la puerta de enlace)"
check "ping IPv4 a→h (192.168.50.20)" ip netns exec a ping -c 3 -W 3 192.168.50.20
check "ping IPv6 a→h (fd50::20)" ip netns exec a ping -6 -c 3 -W 3 fd50::20
ip netns exec h python3 "$TOOLS" tcp-server 192.168.50.20 9000 > $LAB/srv4.json &
S=$!; sleep 0.5
ip netns exec a python3 "$TOOLS" tcp-client 192.168.50.20 9000 1000000 7 > $LAB/cli4.json
wait $S
expect_eq "TCP 1 MB íntegro a→h por IPv4" "$(python3 -c "import json;print(json.load(open('$LAB/srv4.json'))['sha256'])")" "$(python3 -c "import json;print(json.load(open('$LAB/cli4.json'))['sha256'])")"
ip netns exec h python3 "$TOOLS" tcp-server fd50::20 9001 > $LAB/srv6.json &
S=$!; sleep 0.5
ip netns exec a python3 "$TOOLS" tcp-client fd50::20 9001 1000000 8 > $LAB/cli6.json
wait $S
expect_eq "TCP 1 MB íntegro a→h por IPv6" "$(python3 -c "import json;print(json.load(open('$LAB/srv6.json'))['sha256'])")" "$(python3 -c "import json;print(json.load(open('$LAB/cli6.json'))['sha256'])")"
check "sin SNAT no existe la tabla nftables del agente" bash -c "! ip netns exec r nft list table inet atenea_atenea0"

section "ACL conservada para la subred"
D0=$(ctr a tx_acl_denied)
ip netns exec a python3 "$TOOLS" send-udp 192.168.50.20 9000 "udp no permitido"
sleep 1
expect_eq "UDP a la LAN descartado en origen por la ACL" "$(( $(ctr a tx_acl_denied) - D0 ))" 1
check "TCP a un puerto no permitido (9500) no conecta" bash -c "! ip netns exec a timeout 4 python3 $TOOLS tcp-client 192.168.50.20 9500 10 1"
check "destino fuera de la ruta aprobada (192.168.60.1) sin ruta" bash -c '! ip netns exec a ping -c 1 -W 1 192.168.60.1'
S0=$(ctr a tx_bad_source)
ip netns exec a python3 "$TOOLS" spoof 192.168.50.20 10.77.0.2 9000
sleep 1
expect_eq "origen falsificado (IP de la VM) descartado por a" "$(( $(ctr a tx_bad_source) - S0 ))" 1

section "Retorno ausente: sin SNAT no responde; con SNAT=true sí (decisión explícita por ruta)"
check "h2 (sin ruta de retorno) no responde sin SNAT" bash -c '! ip netns exec a ping -c 2 -W 2 192.168.50.30'
spec_router true; issue r
wait_for 25 bash -c "ip netns exec r nft list table inet atenea_atenea0"
check "con snat=true aparece la tabla nftables propia del agente" bash -c "ip netns exec r nft list table inet atenea_atenea0 | grep -q masquerade"
check "h2 responde con SNAT (sin tocar la LAN)" ip netns exec a ping -c 3 -W 3 192.168.50.30
ip netns exec h2 timeout 8 tcpdump -i eth0 -n -c 2 icmp > $LAB/h2.cap 2>/dev/null &
CAP=$!; sleep 1
ip netns exec a ping -c 2 -W 2 192.168.50.30 >/dev/null 2>&1
wait $CAP 2>/dev/null
check "h2 ve el origen enmascarado (192.168.50.1), no el cliente real" bash -c "grep -q '192.168.50.1 > 192.168.50.30' $LAB/h2.cap && ! grep -q '10.77.0' $LAB/h2.cap"
spec_router false; issue r
wait_for 25 bash -c "! ip netns exec r nft list table inet atenea_atenea0"
check "volver a snat=false retira la tabla nftables" bash -c "! ip netns exec r nft list table inet atenea_atenea0"

section "Subredes solapadas: la LAN local gana, la ruta remota queda en conflicto"
wait_for 20 test "$(field a2 config.lease_active)" = true
check "a2 no instaló la ruta remota por atenea0" bash -c "! ip -n a2 route show 192.168.50.0/24 | grep -q 'dev atenea0'"
check "a2 conserva su LAN local (192.168.50.0/24 por lan0)" bash -c "ip -n a2 route show 192.168.50.0/24 | grep -q 'dev lan0'"
expect_eq "estado de la ruta en a2" "$(T field $LAB/a2.sock routes.0.state 2>/dev/null)" conflict
check "a2 alcanza a su vecino local h3 (192.168.50.99)" ip netns exec a2 ping -c 2 -W 2 192.168.50.99

section "Control local: un usuario sin permiso no gobierna el agente"
NOBODY="setpriv --reuid=65534 --regid=65534 --clear-groups"
chmod 755 $LAB
check "un usuario normal NO puede parar al agente por el socket de control" bash -c "! $NOBODY atenea down --socket $LAB/r.sock"
check "un usuario normal NO puede leer el estado por el socket de control" bash -c "! $NOBODY atenea status --socket $LAB/r.sock"
check "un usuario fuera del grupo NO lee el socket de estado" bash -c "! $NOBODY atenea status --socket $LAB/r.status.sock"
check "un usuario del grupo (4242) SÍ lee el estado" bash -c "setpriv --reuid=65534 --regid=4242 --groups 4242 atenea status --socket $LAB/r.status.sock | grep -q public_key"
check "un usuario del grupo NO puede parar por el socket de estado" bash -c "! setpriv --reuid=65534 --regid=4242 --groups 4242 atenea down --socket $LAB/r.status.sock"
check "el agente r sigue activo tras los intentos" ip netns exec a ping -c 1 -W 3 192.168.50.20
expect_eq "modo del socket de control" "$(stat -c %a $LAB/r.sock)" 600
expect_eq "modo del socket de estado" "$(stat -c %a $LAB/r.status.sock)" 660

section "Retirada de la ruta: una configuración nueva sin ella la elimina"
spec_client a 1 0; issue a   # sin rutas ni las reglas de prefijo que dependen de ellas (el verificador las exige)
wait_for 25 bash -c "! ip -n a route show 192.168.50.0/24 | grep -q atenea0"
check "a retira la ruta de la subred" bash -c "! ip -n a route show 192.168.50.0/24 | grep -q atenea0"
check "sin ruta, a ya no alcanza la VM" bash -c '! ip netns exec a ping -c 1 -W 1 192.168.50.20'
spec_client a 1; issue a
wait_for 25 bash -c "ip -n a route show 192.168.50.0/24 | grep -q atenea0"
check "una configuración posterior vuelve a instalarla" bash -c "ip -n a route show 192.168.50.0/24 | grep -q atenea0"

section "Sin payload en claro en la red subyacente"
MARK=ATENEA-SUBRED-9f3c1d
ip netns exec h python3 "$TOOLS" tcp-server 192.168.50.20 9002 > $LAB/mark.json &
S=$!; sleep 0.5
ip netns exec a python3 -c "
import socket
s=socket.create_connection(('192.168.50.20',9002));s.sendall(('$MARK'*100).encode());s.close()"
wait $S
kill -INT $TCPDUMP; wait $TCPDUMP 2>/dev/null
expect_eq "el marcador llegó completo a la VM (bytes)" "$(python3 -c "import json;print(json.load(open('$LAB/mark.json'))['bytes'])")" "$(( ${#MARK} * 100 ))"
expect_eq "el marcador no aparece en la captura del puente subyacente" "$(T contains $LAB/cap.pcap $MARK)" 0

section "Restauración: caída brusca (kill -9), limpieza y parada limpia"
# SNAT activo para comprobar la tabla huérfana
spec_router true; issue r
wait_for 25 bash -c "ip netns exec r nft list table inet atenea_atenea0"
PID_R=$(pgrep -f "atenea-agent run --config $LAB/r.toml" | head -1)
PID_A=$(pgrep -f "atenea-agent run --config $LAB/a.toml" | head -1)
kill -9 $PID_A
sleep 1
check "kill -9 de a: la interfaz atenea0 desaparece con el proceso" bash -c '! ip -n a link show atenea0'
check "kill -9 de a: sin rutas de la subred residuales" bash -c "! ip -n a route show | grep -q 192.168.50.0"
kill -9 $PID_R
sleep 1
check "kill -9 de r: la tabla nftables queda huérfana (es lo esperado)" bash -c "ip netns exec r nft list table inet atenea_atenea0"
ip netns exec r atenea-agent cleanup --config $LAB/r.toml
check "atenea-agent cleanup elimina la tabla huérfana" bash -c "! ip netns exec r nft list table inet atenea_atenea0"
check "cleanup es idempotente (tabla ausente = éxito)" ip netns exec r atenea-agent cleanup --config $LAB/r.toml
rm -f $LAB/r.sock $LAB/a.sock $LAB/r.status.sock
up r; up a
wait_for 30 test "$(field a config.lease_active)" = true
wait_for 30 bash -c "ip -n a route show 192.168.50.0/24 | grep -q atenea0"
check "tras reiniciar, el cliente recupera la ruta" bash -c "ip -n a route show 192.168.50.0/24 | grep -q atenea0"
check "tras reiniciar, vuelve el acceso a la VM" ip netns exec a ping -c 3 -W 3 192.168.50.30
down a; down r
check "parada limpia: sin tabla nftables" bash -c "! ip netns exec r nft list table inet atenea_atenea0"
check "parada limpia: sin interfaz en a" bash -c '! ip -n a link show atenea0'

rm -f $LAB/issuing; kill $ISSUER 2>/dev/null; kill $TCPDUMP 2>/dev/null
echo
if [ $FAILS -eq 0 ]; then echo "LABORATORIO SUBREDES: TODO OK"; else echo "LABORATORIO SUBREDES: $FAILS fallo(s)"; fi
exit $FAILS
