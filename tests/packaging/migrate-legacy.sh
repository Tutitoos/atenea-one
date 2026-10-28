#!/usr/bin/env bash
# Copyright 2026 Tutitoos
# SPDX-License-Identifier: Apache-2.0
# Se carga desde in-container.sh tras multi-network.sh: la instancia heredada sigue activa con tráfico firmado.
# Ejercita `atenea network legacy|migrate-legacy` con systemd y dpkg reales (ADR-013 §10).
set -euo pipefail
net_m=${net:0:8}-${net:8:4}-${net:12:4}-${net:16:4}-${net:20:12}
unit_m="atenea-network@$net_m.service"
run_m=/run/atenea/${net_m: -8}
[ "$(atenea network legacy --json)" = '{"present":true}' ] || fail 'legacy --json no detecta la heredada'
# La heredada de este ensayo no define servidor (la sincronización queda fuera del laboratorio sin red):
# se añade una URL sintética solo ahora; la instancia en marcha no relee el fichero.
printf '[sync]\nurl = "https://control.example.test"\n' >> /etc/atenea/agent.toml
cp /etc/atenea/agent.toml /tmp/legacy-before.toml
# Sin --yes y sin terminal se niega y no cambia nada.
if atenea network migrate-legacy --name sintetica < /dev/null; then fail 'migró sin confirmación'; fi
systemctl is-active --quiet atenea-agent || fail 'la negativa tocó la heredada'
[ -f /etc/atenea/agent.toml ] || fail 'la negativa borró agent.toml'
# Nombre repetido: validación previa, sin parar nada.
if atenea network --backend system --root "$root_a" migrate-legacy --yes --name "sintetica-${net_a: -8}"; then fail 'nombre duplicado aceptado'; fi
systemctl is-active --quiet atenea-agent || fail 'una validación fallida paró la heredada'
atenea network migrate-legacy --yes --name sintetica
# La heredada queda parada, deshabilitada y sin configuración; la red la sustituye con servicio por red.
if systemctl is-active --quiet atenea-agent; then fail 'la heredada sigue activa tras migrar'; fi
if systemctl is-enabled --quiet atenea-agent; then fail 'la heredada sigue habilitada tras migrar'; fi
[ ! -e /etc/atenea/agent.toml ] || fail 'agent.toml heredado sigue ahí'
[ ! -e /var/lib/atenea/device/identity.key ] || fail 'copia de clave fuera del directorio nuevo'
[ ! -e /var/lib/atenea/config.bin ] || fail 'config.bin no se movió'
[ ! -e /var/lib/atenea/device/noise.key ] || fail 'copia de clave fuera del directorio nuevo'
migrated="/var/lib/atenea/networks/$net_m"
assert_mode "$migrated" '711 root root'
assert_mode "$migrated/device" '700 root root'
assert_mode "$migrated/device/noise.key" '600 root root'
assert_mode "$migrated/legacy-agent.toml.bak" '600 root root'
cmp /tmp/legacy-before.toml "$migrated/legacy-agent.toml.bak" || fail 'el respaldo no es el agent.toml heredado'
systemctl is-active --quiet "$unit_m" || fail 'la red migrada no está activa por atenea-network@UUID'
systemctl is-enabled --quiet "$unit_m" || fail 'la red migrada no está habilitada'
wait_traffic 10.77.0.2
[ "$(python3 tests/network-lab/tools.py field "$run_m/status.sock" config.lease_active)" = true ] || fail 'la red migrada no tiene configuración vigente'
assert_mode "$run_m/control.sock" '600 root atenea'
assert_mode "$run_m/status.sock" '660 root atenea'
[ ! -S /run/atenea/control.sock ] || fail 'queda el socket de control heredado'
[ ! -S /run/atenea/status.sock ] || fail 'queda el socket de estado heredado'
ip link show atenea0 >/dev/null || fail 'la red migrada perdió su interfaz'
[ "$(atenea network legacy --json)" = '{"present":false}' ] || fail 'legacy --json sigue detectando heredada'
python3 - "$net_m" <<'PY'
import json, subprocess, sys
nets = json.loads(subprocess.check_output(['atenea', 'network', 'list', '--json']))
net = next(n for n in nets if n['network_id'] == sys.argv[1])
assert net['state'] == 'conectada' and net['backend'] == 'systemd' and net['name'] == 'sintetica', net
assert net['interface'] == 'atenea0' and net['listen_port'] == 51820 and net['join_status'] == 'aprobada', net
PY
# Idempotente: repetir no cambia nada y avisa con claridad.
if atenea network migrate-legacy --yes 2> /tmp/migrate-again.err; then fail 'segunda migración aceptada'; fi
grep -q 'no hay instalación heredada' /tmp/migrate-again.err || fail 'error poco claro en la segunda migración'
# La heredada NO vuelve: ni al arrancarla a mano ni tras reiniciar systemd.
systemctl start atenea-agent || true
sleep 1
if systemctl is-active --quiet atenea-agent; then fail 'la heredada arrancó sin agent.toml'; fi
systemctl daemon-reexec
sleep 1
if systemctl is-active --quiet atenea-agent; then fail 'la heredada volvió tras daemon-reexec'; fi
wait_traffic 10.77.0.2
# Una actualización del paquete no resucita la heredada y conserva la red migrada (postinst/prerm).
dpkg -i /tmp/atenea.deb
if systemctl is-active --quiet atenea-agent; then fail 'la actualización arrancó la heredada'; fi
wait_traffic 10.77.0.2
systemctl is-active --quiet "$unit_m" || fail 'la actualización paró la red migrada'
# Desde aquí remove/purge se comparan contra el estado migrado.
find "$migrated" "$root_a/$net_a" "$root_b/$net_b" -type f \( -name '*.key' -o -name agent.toml -o -name network.json -o -name bundle.bin \) -print0 | sort -z | xargs -0 sha256sum > "$lab/preserved.sha256"
echo 'Migración de la heredada (systemd+dpkg reales): red por atenea-network@UUID, heredada inactiva y sin retorno: OK'
