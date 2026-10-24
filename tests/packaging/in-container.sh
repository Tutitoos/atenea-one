#!/usr/bin/env bash
set -euo pipefail
fail() { echo "FALLO: $*" >&2; journalctl -u atenea-agent --no-pager -n 60; exit 1; }
wait_active() {
    for ((i=0; i<50; i++)); do
        if systemctl is-active --quiet atenea-agent && [ -S /run/atenea/status.sock ]; then return; fi
        sleep 0.2
    done
    fail 'servicio o socket no disponible'
}
assert_mode() { [ "$(stat -c '%a %U %G' "$1")" = "$2" ] || fail "permisos de $1"; }
cd /src
bash -n packaging/deb/build.sh packaging/deb/postinst packaging/deb/prerm packaging/deb/postrm tests/packaging/run.sh tests/packaging/in-container.sh
shellcheck packaging/deb/build.sh packaging/deb/postinst packaging/deb/prerm packaging/deb/postrm tests/packaging/run.sh tests/packaging/in-container.sh
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
network_prefixes = ["10.77.0.0/24"]
EOF
atenea issue --issuer-key /tmp/authority/issuer/issuer.key --issuer-cert /tmp/authority/issuer/issuer.cert --spec /tmp/spec.toml --out /var/lib/atenea/config.bin
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
systemctl start atenea-agent
wait_active
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
# Simula una tabla residual tras SIGKILL; cleanup debe eliminarla antes del reinicio.
nft add table inet atenea_atenea0
nft add chain inet atenea_atenea0 residuo
old_pid=$(systemctl show atenea-agent -p MainPID --value)
old_ifindex=$(cat /sys/class/net/atenea0/ifindex)
kill -9 "$old_pid"
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
# Prueba real de resolved por enlace; la integración firmada se cubre en el E2E de subredes.
if systemctl start systemd-resolved && resolvectl status >/dev/null 2>&1; then
    ip link add dnsprueba type dummy
    ip link set dnsprueba up
    resolvectl dns dnsprueba 10.77.0.1
    resolvectl domain dnsprueba '~lab.internal'
    resolvectl dns dnsprueba | python3 -c 'import sys; assert "10.77.0.1" in sys.stdin.read()'
    resolvectl domain dnsprueba | python3 -c 'import sys; assert "~lab.internal" in sys.stdin.read()'
    ip link delete dnsprueba
    # `resolvectl status <enlace inexistente>` sale con 0 y solo avisa: se mira la lista completa de enlaces.
    if resolvectl status 2>/dev/null | grep -q '(dnsprueba)'; then fail 'resolved conserva el enlace borrado'; fi
    echo 'DNS real por enlace y retirada: OK (enlace dummy, no integración del agente)'
else
    echo 'DNS NO comprobado: systemd-resolved no puede arrancar en este contenedor' >&2
fi
dpkg -r atenea
if ip link show atenea0 2>/dev/null; then fail 'interfaz residual al desinstalar'; fi
if nft list table inet atenea_atenea0 2>/dev/null; then fail 'tabla residual al desinstalar'; fi
[ ! -S /run/atenea/control.sock ]
[ ! -S /run/atenea/status.sock ]
dpkg -P atenea
[ -f /var/lib/atenea/device/noise.key ]
[ -f /etc/atenea/agent.toml ]
echo 'Empaquetado, permisos, reinicio y desinstalación: OK'
