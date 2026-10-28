#!/usr/bin/env bash
# Copyright 2026 Tutitoos
# SPDX-License-Identifier: Apache-2.0
set -euo pipefail
export LC_ALL=C SYSTEMD_COLORS=0
# La imagen offline de infraestructura no incluye resolved ni shellcheck. El modo explícito conserva
# las pruebas de servicio/red/empaquetado; jamás presenta las comprobaciones DNS omitidas como verdes.
no_dns=${ATENEA_PACKAGING_NO_DNS:-0}
fail() { echo "FALLO: $*" >&2; journalctl -u atenea-agent --no-pager -n 60; exit 1; }
wait_active() {
    for ((i=0; i<50; i++)); do
        if systemctl is-active --quiet atenea-agent && [ -S /run/atenea/status.sock ] \
            && [ "$(status_field config.lease_active 2>/dev/null)" = true ]; then return; fi
        sleep 0.2
    done
    fail 'servicio o socket no disponible'
}
assert_mode() { [ "$(stat -c '%a %U %G' "$1")" = "$2" ] || fail "permisos de $1"; }
status_field() { python3 tests/network-lab/tools.py field /run/atenea/status.sock "$1"; }
wait_dns() {
    for ((i=0; i<50; i++)); do
        if [ "$(status_field dns 2>/dev/null)" = "$1" ]; then return; fi
        sleep 0.2
    done
    fail "estado DNS distinto de $1"
}
assert_resolv_conf() {
    [ "$(sha256sum /etc/resolv.conf)" = "$resolv_conf_before" ] || fail '/etc/resolv.conf modificado'
}
assert_dns() {
    if [ "$no_dns" = 1 ]; then wait_dns none; return; fi
    wait_dns applied
    resolvectl status atenea0 | tee /tmp/resolved-link.status
    resolvectl dns atenea0 | tee /tmp/resolved-link.dns
    resolvectl domain atenea0 | tee /tmp/resolved-link.domain
    python3 - <<'PY'
from pathlib import Path
assert Path('/tmp/resolved-link.dns').read_text().split(':', 1)[1].split() == ['10.77.0.1']
assert Path('/tmp/resolved-link.domain').read_text().split(':', 1)[1].split() == ['~lab.internal']
status = Path('/tmp/resolved-link.status').read_text()
assert '-DefaultRoute' in status or 'Default Route: no' in status, status
assert '~.' not in status.split(), status
PY
    [ "$(resolvectl default-route atenea0 | cut -d: -f2 | xargs)" = no ] || fail 'enlace como ruta DNS predeterminada'
    assert_resolv_conf
}
wait_removed() {
    for ((i=0; i<100; i++)); do
        if [ "$no_dns" != 1 ]; then resolvectl status > /tmp/resolved-all.status; else : > /tmp/resolved-all.status; fi
        if [ ! -e /sys/class/net/atenea0 ] && ! grep -q '(atenea0)' /tmp/resolved-all.status; then
            cat /tmp/resolved-all.status
            assert_resolv_conf
            return
        fi
        sleep 0.02
    done
    fail 'interfaz o configuración DNS residual en resolved'
}
cd /src
bash -n packaging/deb/build.sh packaging/deb/postinst packaging/deb/prerm packaging/deb/postrm packaging/systemd/network-agent tests/packaging/run.sh tests/packaging/in-container.sh tests/packaging/multi-network.sh
if command -v shellcheck >/dev/null; then
    shellcheck packaging/deb/build.sh packaging/deb/postinst packaging/deb/prerm packaging/deb/postrm packaging/systemd/network-agent tests/packaging/run.sh tests/packaging/in-container.sh tests/packaging/multi-network.sh
elif [ "$no_dns" = 1 ]; then
    echo 'NO COMPROBADO: shellcheck no está en la imagen offline.'
else
    fail 'falta shellcheck'
fi
bash packaging/deb/build.sh /cache/target/release 0.7.0 "$(dpkg --print-architecture)" /tmp/atenea.deb
dpkg-deb --info /tmp/atenea.deb
dpkg-deb --contents /tmp/atenea.deb
dpkg -i /tmp/atenea.deb
getent group atenea
[ "$(systemctl show atenea-agent -p User --value)" = root ]
if getent passwd atenea; then fail 'usuario atenea inesperado: el servicio debe usar root'; fi
if systemctl is-active --quiet atenea-agent; then fail 'arranque automático al instalar'; fi
if systemctl is-enabled --quiet atenea-agent; then fail 'habilitación automática al instalar'; fi
assert_mode /etc/atenea '750 root atenea'
assert_mode /var/lib/atenea '700 root atenea'
systemd-analyze verify /usr/lib/systemd/system/atenea-agent.service
systemd-analyze security --offline=yes /usr/lib/systemd/system/atenea-agent.service
# Claves sintéticas efímeras, generadas dentro del contenedor y nunca copiadas al repo.
umask 077
mkdir /tmp/authority
atenea root-init --out /tmp/authority/root.key > /tmp/authority/root.pub
net=00112233445566778899aabbccddeeff
atenea issuer-init --root /tmp/authority/root.key --network-id "$net" --serial 1 --out-dir /tmp/authority/issuer
atenea device-init --dir /var/lib/atenea/device > /tmp/device.info
device=$(python3 -c 'from pathlib import Path; print(dict(line.split("=",1) for line in Path("/tmp/device.info").read_text().splitlines() if "=" in line)["device_id"])')
cat > /tmp/spec.toml <<EOF
network_id = "$net"
device_bundle = "/var/lib/atenea/device/bundle.bin"
validity_secs = 3600
tunnel_addrs = ["10.77.0.1"]
network_prefixes = ["10.77.0.0/25"]
[[advertise]]
prefix = "10.77.0.128/25"
snat = false
EOF
cp /tmp/spec.toml /tmp/spec-without-dns.toml
cat >> /tmp/spec.toml <<'EOF'
[dns]
servers = ["10.77.0.1"]
domains = ["lab.internal"]
EOF
issue_config() {
    atenea issue --issuer-key /tmp/authority/issuer/issuer.key --issuer-cert /tmp/authority/issuer/issuer.cert --spec "$1" --out /var/lib/atenea/config.bin
}
if [ "$no_dns" = 1 ]; then issue_config /tmp/spec-without-dns.toml; else issue_config /tmp/spec.toml; fi
cat > /etc/atenea/agent.toml <<EOF
interface = "atenea0"
listen = "0.0.0.0:51820"
ipc_socket = "/run/atenea/control.sock"
status_socket = "/run/atenea/status.sock"
status_gid = $(getent group atenea | cut -d: -f3)
noise_key_file = "/var/lib/atenea/device/noise.key"
identity_key_file = "/var/lib/atenea/device/identity.key"
config_file = "/var/lib/atenea/config.bin"
state_dir = "/var/lib/atenea/state"
network_id = "$net"
device_id = "$device"
trust_anchor = "$(cat /tmp/authority/root.pub)"
EOF
if [ "$no_dns" != 1 ]; then
    systemctl start systemd-resolved
    resolvectl status
else
    echo 'NO COMPROBADO: DNS dividido/systemd-resolved (modo offline sin resolved).'
fi
resolv_conf_before=$(sha256sum /etc/resolv.conf)
echo 0 > /proc/sys/net/ipv4/ip_forward
systemctl start atenea-agent
wait_active
assert_dns
[ "$(status_field forwarding_ok)" = false ] || fail 'forwarding_ok no informa false con ip_forward=0'
[ "$(cat /proc/sys/net/ipv4/ip_forward)" = 0 ] || fail 'el agente activó ip_forward'
echo 1 > /proc/sys/net/ipv4/ip_forward
# El estado del agente es una instantánea que se renueva cada segundo: se espera, no se supone.
for _ in $(seq 1 10); do [ "$(status_field forwarding_ok)" = true ] && break; sleep 1; done
[ "$(status_field forwarding_ok)" = true ] || fail 'forwarding_ok no se actualiza con ip_forward=1'
echo 'Configuración firmada y forwarding declarado: OK'
assert_mode /run/atenea '750 root atenea'
assert_mode /run/atenea/control.sock '600 root atenea'
assert_mode /run/atenea/status.sock '660 root atenea'
useradd --no-create-home --shell /bin/sh ajeno
useradd --no-create-home --shell /bin/sh --groups atenea lector
if runuser -u ajeno -- atenea status --socket /run/atenea/status.sock; then fail 'estado accesible fuera del grupo'; fi
runuser -u lector -- atenea status --socket /run/atenea/status.sock
if runuser -u lector -- atenea status --socket /run/atenea/control.sock; then fail 'control accesible al grupo'; fi
# El socket de estado debe rechazar ambas mutaciones incluso para un miembro del grupo.
runuser -u lector -- python3 - <<'PY'
import socket
for command in (b"stop\n", b"reload\n"):
    with socket.socket(socket.AF_UNIX) as stream:
        stream.settimeout(5)
        stream.connect('/run/atenea/status.sock')
        stream.sendall(command)
        response = stream.recv(65536)
        assert b'error' in response, response
PY
systemctl is-active --quiet atenea-agent
systemctl stop atenea-agent
wait_removed
systemctl start atenea-agent
wait_active
assert_dns
echo 'Parada limpia y reaplicación de configuración por systemd: OK'
# Amplía solo la ventana de observación de la caída; no cambia el reinicio on-failure del paquete.
mkdir -p /run/systemd/system/atenea-agent.service.d
printf '[Service]\nRestartSec=10s\n' > /run/systemd/system/atenea-agent.service.d/observar-caida.conf
systemctl daemon-reload
# Simula una tabla residual tras SIGKILL; cleanup debe eliminarla antes del reinicio.
nft add table inet atenea_atenea0
nft add chain inet atenea_atenea0 residuo
old_pid=$(systemctl show atenea-agent -p MainPID --value)
old_ifindex=$(cat /sys/class/net/atenea0/ifindex)
kill -9 "$old_pid"
wait_removed
for ((i=0; i<100; i++)); do
    new_pid=$(systemctl show atenea-agent -p MainPID --value)
    if [ "$new_pid" != 0 ] && [ "$new_pid" != "$old_pid" ] && [ -S /run/atenea/status.sock ]; then break; fi
    sleep 0.2
done
wait_active
[ "$new_pid" != "$old_pid" ] || fail 'no reinició tras SIGKILL'
[ "$(cat /sys/class/net/atenea0/ifindex)" != "$old_ifindex" ] || fail 'interfaz antigua persistente'
if nft list chain inet atenea_atenea0 residuo 2>/dev/null; then fail 'tabla residual tras reinicio'; fi
atenea status --socket /run/atenea/status.sock
assert_dns
echo 'SIGKILL: enlace retirado; reinicio por systemd reaplica configuración: OK'
if [ "$no_dns" != 1 ]; then
issue_config /tmp/spec-without-dns.toml
atenea reload --socket /run/atenea/control.sock
wait_dns none
resolvectl status atenea0
resolvectl dns atenea0 | tee /tmp/resolved-without-dns
resolvectl domain atenea0 | tee /tmp/resolved-without-domain
python3 - <<'PY'
from pathlib import Path
for name in ('dns', 'domain'):
    output = Path('/tmp/resolved-without-' + name).read_text()
    assert not output.split(':', 1)[1].split(), output
PY
assert_resolv_conf
echo 'Configuración posterior sin DNS: dns=none y enlace sin servidores ni dominios: OK'
fi
# shellcheck source=tests/packaging/multi-network.sh
source tests/packaging/multi-network.sh
dpkg -r atenea
if ip link show atenea0 2>/dev/null; then fail 'interfaz residual al desinstalar'; fi
if nft list table inet atenea_atenea0 2>/dev/null; then fail 'tabla residual al desinstalar'; fi
[ ! -S /run/atenea/control.sock ]
[ ! -S /run/atenea/status.sock ]
assert_networks_removed
dpkg -P atenea
[ -f /var/lib/atenea/device/noise.key ]
[ -f /etc/atenea/agent.toml ]
assert_networks_removed
echo 'Empaquetado, permisos, reinicio y desinstalación: OK'
