#!/usr/bin/env bash
# Copyright 2026 Tutitoos
# SPDX-License-Identifier: Apache-2.0
# Se carga desde in-container.sh: systemd es PID 1 y el legacy ya está probado y activo.
set -euo pipefail
[ "$(cat /proc/1/comm)" = systemd ] || fail 'systemd no es PID 1'
net_a=00112233-4455-6677-8899-aabb00000001
net_b=00112233-4455-6677-8899-aabb00000002
unit_a="atenea-network@$net_a.service"
unit_b="atenea-network@$net_b.service"
root_a=/var/lib/atenea/networks
# Un --root real fuera del StateDirectory de la plantilla prueba el dropin y el sandbox.
root_b=/opt/atenea-sintetico/networks
lab=/var/lib/atenea/packaging-lab
mkdir -p "$lab"
[ -x /usr/lib/atenea/network-agent ] || fail 'falta lanzador de red'
if /usr/lib/atenea/network-agent run '../incorrecto'; then fail 'UUID inválido aceptado'; fi
if /usr/lib/atenea/network-agent run "$net_a" relativa.toml; then fail 'ruta relativa aceptada'; fi
systemd-analyze verify /usr/lib/systemd/system/atenea-network@.service
# La plantilla conserva exactamente todas las restricciones de la unidad legacy.
python3 - <<'PY'
from pathlib import Path
import configparser

def service(name):
    parser = configparser.ConfigParser(interpolation=None, strict=False)
    parser.optionxform = str
    parser.read('/usr/lib/systemd/system/' + name)
    return dict(parser['Service'])
legacy = service('atenea-agent.service')
network = service('atenea-network@.service')
for key, value in legacy.items():
    if key not in {'ExecStart', 'ExecStopPost'}:
        assert network.get(key) == value, (key, value, network.get(key))
PY
# Sin configuración la condición impide arrancar, sin crear interfaz alguna.
systemctl start "atenea-network@$net_a.service"
if systemctl is-active --quiet "$unit_a"; then fail 'arrancó sin agent.toml'; fi
ip link add pkgbr type bridge
ip addr add 192.0.2.1/24 dev pkgbr
ip link set pkgbr up

metadata() {
    python3 - "$1" "$2" "$3" "$4" <<'PY'
import json, sys
from pathlib import Path
root, net, iface, port = sys.argv[1:]
(root_path := Path(root) / net).mkdir(parents=True, exist_ok=True)
(root_path / 'network.json').write_text(json.dumps(dict(
    network_id=net, name='sintetica-' + net[-8:], server='192.0.2.1',
    url='https://192.0.2.1', root_anchor_fingerprint='00' * 32,
    device_id='00' * 16, device_fingerprint='00' * 32,
    interface=iface, listen_port=int(port), created=1, join_status='aprobada')))
PY
}

# fixture <nombre> <uuid-hex> <device-cliente> <config-cliente> <ip-cliente> <ip-peer> <prefijo> <puerto>
fixture() {
    local name=$1 network=$2 client=$3 config=$4 local_ip=$5 peer_ip=$6 prefix=$7 port=$8
    local peer="$lab/$name" idx
    case "$name" in legacy) idx=2 ;; a) idx=3 ;; b) idx=4 ;; esac
    mkdir -p "$peer"
    atenea issuer-init --root /tmp/authority/root.key --network-id "$network" --serial 1 --out-dir "$peer/issuer"
    atenea device-init --dir "$peer/device" > "$peer/device.info"
    ip netns add "pkg-$name"
    ip link add "pkg-$name-host" type veth peer name "pkg-$name-peer"
    ip link set "pkg-$name-peer" netns "pkg-$name"
    ip link set "pkg-$name-host" master pkgbr up
    ip -n "pkg-$name" link set lo up
    ip -n "pkg-$name" addr add "192.0.2.$idx/24" dev "pkg-$name-peer"
    ip -n "pkg-$name" link set "pkg-$name-peer" up
    for side in client peer; do
        local own other address other_address endpoint out
        if [ "$side" = client ]; then
            own=$client; other="$peer/device"; address=$local_ip; other_address=$peer_ip
            endpoint="192.0.2.$idx:51820"; out=$config
        else
            own="$peer/device"; other=$client; address=$peer_ip; other_address=$local_ip
            endpoint="192.0.2.1:$port"; out="$peer/config.bin"
        fi
        # La política es sin estado: permitir TCP también hacia el puerto efímero de las respuestas.
        cat > "$peer/$side.spec.toml" <<EOF
network_id = "$network"
device_bundle = "$own/bundle.bin"
validity_secs = 3600
tunnel_addrs = ["$address"]
network_prefixes = ["$prefix"]
[[peer]]
bundle = "$other/bundle.bin"
addrs = ["$other_address"]
endpoints = ["$endpoint"]
[[rule]]
src = "any"
dst = "any"
proto = "tcp"
EOF
        atenea issue --issuer-key "$peer/issuer/issuer.key" --issuer-cert "$peer/issuer/issuer.cert" --spec "$peer/$side.spec.toml" --out "$out"
    done
    local device
    device=$(python3 -c 'from pathlib import Path; import sys; print(dict(x.split("=",1) for x in Path(sys.argv[1]).read_text().splitlines() if "=" in x)["device_id"])' "$peer/device.info")
    cat > "$peer/agent.toml" <<EOF
interface = "atenea0"
listen = "0.0.0.0:51820"
ipc_socket = "$peer/control.sock"
noise_key_file = "$peer/device/noise.key"
identity_key_file = "$peer/device/identity.key"
config_file = "$peer/config.bin"
state_dir = "$peer/state"
network_id = "$network"
device_id = "$device"
trust_anchor = "$(cat /tmp/authority/root.pub)"
EOF
    systemd-run --unit="packaging-peer-$name" -- ip netns exec "pkg-$name" /usr/bin/atenea-agent run --config "$peer/agent.toml"
    local ready=no
    for ((attempt=0; attempt<50; attempt++)); do
        if ip -n "pkg-$name" -o addr show dev atenea0 2>/dev/null | grep -F "$peer_ip/" | grep -qv ' tentative '; then ready=yes; break; fi
        sleep 0.2
    done
    [ "$ready" = yes ] || fail "peer firmado $name no configuró el túnel"
    systemd-run --unit="packaging-traffic-$name" -- ip netns exec "pkg-$name" python3 /src/tests/packaging/traffic.py serve "$peer_ip"
}

network_fixture() {
    local root=$1 uuid=$2 name=$3 iface=$4 port=$5 local_ip=$6 peer_ip=$7 prefix=$8
    local device="$root/$uuid/device" id
    metadata "$root" "$uuid" "$iface" "$port"
    atenea device-init --dir "$device" > "$lab/$name.info"
    id=$(python3 -c 'from pathlib import Path; import sys; print(dict(x.split("=",1) for x in Path(sys.argv[1]).read_text().splitlines() if "=" in x)["device_id"])' "$lab/$name.info")
    fixture "$name" "${uuid//-/}" "$device" "$device/config.bin" "$local_ip" "$peer_ip" "$prefix" "$port"
    cat > "$device/agent.toml" <<EOF
interface = "$iface"
listen = "0.0.0.0:$port"
ipc_socket = "/run/atenea/${uuid: -8}/control.sock"
status_socket = "/run/atenea/${uuid: -8}/status.sock"
status_gid = $(getent group atenea | cut -d: -f3)
noise_key_file = "$device/noise.key"
identity_key_file = "$device/identity.key"
config_file = "$device/config.bin"
state_dir = "$device/state"
network_id = "${uuid//-/}"
device_id = "$id"
trust_anchor = "$(cat /tmp/authority/root.pub)"
EOF
}
fixture legacy "$net" /var/lib/atenea/device /var/lib/atenea/config.bin 10.77.0.1 10.77.0.2 10.77.0.0/24 51820
atenea reload --socket /run/atenea/control.sock
network_fixture "$root_a" "$net_a" a atenea1 51821 fd77::10:1 fd77::10:2 fd77::10:0/112
network_fixture "$root_b" "$net_b" b atenea2 51822 fd77::20:1 fd77::20:2 fd77::20:0/112
atenea network --backend system --root "$root_a" up "$net_a"
atenea network --backend system --root "$root_b" up "$net_b"
probe() { python3 tests/packaging/traffic.py probe "$1" >/dev/null; }
wait_traffic() {
    for ((attempt=0; attempt<40; attempt++)); do
        if probe "$1" 2>/dev/null; then return; fi
        sleep 0.25
    done
    fail "no hay tráfico firmado hacia $1"
}
all_traffic() { wait_traffic 10.77.0.2; wait_traffic fd77::10:2; wait_traffic fd77::20:2; }
all_traffic
for iface in atenea0 atenea1 atenea2; do ip link show "$iface" || fail "falta interfaz $iface"; done
for suffix in 00000001 00000002; do
    assert_mode "/run/atenea/$suffix" '750 root atenea'
    assert_mode "/run/atenea/$suffix/control.sock" '600 root atenea'
    assert_mode "/run/atenea/$suffix/status.sock" '660 root atenea'
done
for port in 51820 51821 51822; do
    [ -n "$(ss -ulnH "sport = :$port")" ] || fail "falta puerto $port"
done
[ "$(systemctl show "$unit_a" -p MainPID --value)" != "$(systemctl show "$unit_b" -p MainPID --value)" ] || fail 'PIDs compartidos'
# Durante down/restart/disable de A, ni legacy ni B pierden su conexión TCP establecida.
legacy_pid=$(systemctl show atenea-agent -p MainPID --value)
b_pid=$(systemctl show "$unit_b" -p MainPID --value)
legacy_inode=$(stat -c %i /run/atenea/control.sock)
b_inode=$(stat -c %i /run/atenea/00000002/control.sock)
legacy_ifindex=$(cat /sys/class/net/atenea0/ifindex)
b_ifindex=$(cat /sys/class/net/atenea2/ifindex)
python3 tests/packaging/traffic.py watch 10.77.0.2 > "$lab/legacy.traffic" 2> "$lab/legacy.error" & watch_legacy=$!
python3 tests/packaging/traffic.py watch fd77::20:2 > "$lab/b.traffic" 2> "$lab/b.error" & watch_b=$!
atenea network --backend system --root "$root_a" down "$net_a"
[ ! -e /sys/class/net/atenea1 ] || fail 'down dejó interfaz A'
[ ! -S /run/atenea/00000001/control.sock ] || fail 'down dejó socket A'
atenea network --backend system --root "$root_a" up "$net_a"
wait_traffic fd77::10:2
systemctl restart "$unit_a"
wait_traffic fd77::10:2
systemctl disable --now "$unit_a"
sleep 2
[ "$legacy_pid" = "$(systemctl show atenea-agent -p MainPID --value)" ] || fail 'cambió PID legacy'
[ "$b_pid" = "$(systemctl show "$unit_b" -p MainPID --value)" ] || fail 'cambió PID B'
[ "$legacy_inode" = "$(stat -c %i /run/atenea/control.sock)" ] || fail 'cambió socket legacy'
[ "$b_inode" = "$(stat -c %i /run/atenea/00000002/control.sock)" ] || fail 'cambió socket B'
[ "$legacy_ifindex" = "$(cat /sys/class/net/atenea0/ifindex)" ] || fail 'cambió interfaz legacy'
[ "$b_ifindex" = "$(cat /sys/class/net/atenea2/ifindex)" ] || fail 'cambió interfaz B'
for watcher in "$watch_legacy" "$watch_b"; do kill -0 "$watcher" || fail 'se interrumpió tráfico de otra red'; done
kill "$watch_legacy" "$watch_b"
wait "$watch_legacy" 2>/dev/null || true
wait "$watch_b" 2>/dev/null || true
python3 - "$lab" <<'PY'
from pathlib import Path
import sys
for name in ('legacy', 'b'):
    root = Path(sys.argv[1])
    assert not (root / (name + '.error')).read_text()
    lines = (root / (name + '.traffic')).read_text().splitlines()
    assert len(lines) >= 20, (name, lines)
    assert [int(x) for x in lines] == list(range(1, len(lines) + 1))
PY
# Upgrade recuerda activas, no las meramente habilitadas: A sigue parada.
systemctl enable "$unit_a" "$unit_b" atenea-agent.service
old_legacy=$legacy_pid
old_b=$b_pid
dpkg -i /tmp/atenea.deb
wait_traffic 10.77.0.2
wait_traffic fd77::20:2
if systemctl is-active --quiet "$unit_a"; then fail 'upgrade arrancó red inactiva'; fi
[ "$old_legacy" != "$(systemctl show atenea-agent -p MainPID --value)" ] || fail 'upgrade no reinició legacy'
[ "$old_b" != "$(systemctl show "$unit_b" -p MainPID --value)" ] || fail 'upgrade no reinició B'
# Las dos activas y legacy también se recuperan en la siguiente actualización.
atenea network --backend system --root "$root_a" up "$net_a"
all_traffic
old_a=$(systemctl show "$unit_a" -p MainPID --value)
old_b=$(systemctl show "$unit_b" -p MainPID --value)
old_legacy=$(systemctl show atenea-agent -p MainPID --value)
dpkg -i /tmp/atenea.deb
all_traffic
[ "$old_a" != "$(systemctl show "$unit_a" -p MainPID --value)" ] || fail 'upgrade no reinició A'
[ "$old_b" != "$(systemctl show "$unit_b" -p MainPID --value)" ] || fail 'upgrade no reinició B'
[ "$old_legacy" != "$(systemctl show atenea-agent -p MainPID --value)" ] || fail 'upgrade no reinició legacy'
systemctl daemon-reexec
for unit in atenea-agent.service "$unit_a" "$unit_b"; do
    systemctl is-enabled --quiet "$unit" || fail "se perdió habilitación de $unit"
    systemctl is-active --quiet "$unit" || fail "se perdió instancia $unit"
done
all_traffic
# Parar legacy tampoco puede retirar los sockets de las redes.
a_pid=$(systemctl show "$unit_a" -p MainPID --value)
b_pid=$(systemctl show "$unit_b" -p MainPID --value)
systemctl stop atenea-agent
wait_traffic fd77::10:2
wait_traffic fd77::20:2
[ "$a_pid" = "$(systemctl show "$unit_a" -p MainPID --value)" ] || fail 'stop legacy reinició A'
[ "$b_pid" = "$(systemctl show "$unit_b" -p MainPID --value)" ] || fail 'stop legacy reinició B'
systemctl start atenea-agent
all_traffic
# Guardar contenido, no solo existencia: remove/purge no pueden alterar claves ni configuración.
find /var/lib/atenea/device "$root_a/$net_a" "$root_b/$net_b" -type f \( -name '*.key' -o -name agent.toml -o -name network.json -o -name bundle.bin \) -print0 | sort -z | xargs -0 sha256sum > "$lab/preserved.sha256"
assert_networks_removed() {
    for iface in atenea1 atenea2; do
        [ ! -e "/sys/class/net/$iface" ] || fail "interfaz residual $iface"
        if nft list table inet "atenea_$iface" 2>/dev/null; then fail "tabla residual $iface"; fi
    done
    for suffix in 00000001 00000002; do
        [ ! -S "/run/atenea/$suffix/control.sock" ] || fail 'socket control residual'
        [ ! -S "/run/atenea/$suffix/status.sock" ] || fail 'socket estado residual'
    done
    for port in 51820 51821 51822; do
        [ -z "$(ss -ulnH "sport = :$port")" ] || fail "puerto residual $port"
    done
    sha256sum -c "$lab/preserved.sha256"
    for unit in "$unit_a" "$unit_b" atenea-agent.service; do
        if systemctl is-active --quiet "$unit"; then fail "servicio residual $unit"; fi
        if systemctl is-enabled --quiet "$unit"; then fail "habilitación residual $unit"; fi
    done
}
echo 'Multiinstancia systemd, tráfico firmado, aislamiento de ciclo de vida y upgrade: preparado para remove/purge'
