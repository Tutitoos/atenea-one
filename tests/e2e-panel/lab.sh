#!/usr/bin/env bash
# Pasos de laboratorio para el E2E del panel (los invoca la prueba de Playwright con child_process).
# Corre dentro del contenedor de run.sh: PostgreSQL y Dex son contenedores hermanos (`postgres`, `dex`);
# control plane y panel van en el espacio de nombres raíz (IP 198.51.100.254 en el puente público);
# coordinator, relay y HAProxy en `inet` (198.51.100.10); agentes en `a` y `b` tras NAT domésticos.
#
#   lab.sh setup                      topología, servicios y claves (raíz offline en /tmp/lab)
#   lab.sh issuer-cert <uuid-red>     genera /tmp/lab/issuer.cert para esa red
#   lab.sh device-init <a|b|r|c>      claves del dispositivo; imprime device_id y fingerprint
#   lab.sh enroll <n> <url> <token>  enrola el dispositivo (queda pending)
#   lab.sh start-agent <n> <join-json> (r declara 192.168.50.0/24)
#   lab.sh field <a|b> <campo>        campo del estado del agente
#   lab.sh ping <a|b> <ip>            0 si responde
#   lab.sh transfer <ip>              TCP 1 MB a→b; imprime ok/fail
#   lab.sh block <p2p|udp|none>
#   lab.sh routes-setup              router r, cliente c y LAN h sin retorno (forwarding explícito)
#   lab.sh subnet-transfer           TCP 1 MB c→h; comprueba integridad y origen SNAT
#   lab.sh subnet-route              ruta del cliente a la LAN
#   lab.sh dns-log <r|c>              invocaciones al resolvectl simulado
set -uo pipefail
cd "${LAB_ROOT:-$PWD}"

LAB=/tmp/lab
ROOT=${LAB_ROOT:-$PWD}
TOOLS=$ROOT/tests/network-lab/tools.py
CP_IP=198.51.100.254
NET=00112233445566778899aabbccddeeff
export PATH=${CARGO_TARGET_DIR:-/cache/target}/release:$PATH
source "$ROOT/tests/network-lab/nat/lib.sh"

cmd=${1:-}; shift || true
case "$cmd" in
setup)
  rm -rf "$LAB"; mkdir -p "$LAB"
  topo_setup home || exit 1
  ip addr add $CP_IP/24 dev nl-public
  atenea root-init --out $LAB/root.key > $LAB/root.pub
  ROOT_PUB=$(cat $LAB/root.pub)
  bash "$ROOT/tests/network-lab/nat/gen-certs.sh" $LAB/certs > /dev/null || exit 1
  ( umask 077; head -c 32 /dev/urandom | od -An -tx1 | tr -d ' \n' > $LAB/relay-signing.hex
    head -c 32 /dev/urandom | od -An -tx1 | tr -d ' \n' > $LAB/issuer.key )
  RELAY_PUB=$(ATENEA_COORD_RELAY_SIGNING_KEY_FILE=$LAB/relay-signing.hex atenea-coordinator --print-relay-key)

  # Control plane + panel (mismo origen)
  ADMIN_URL=postgres://postgres:pg@postgres/atenea
  psql "$ADMIN_URL" -qc "CREATE ROLE atenea_app LOGIN PASSWORD 'app-pass'" >/dev/null
  export ATENEA_CP_PUBLIC_URL=http://$CP_IP:8080
  export ATENEA_CP_OIDC_ISSUER=http://dex:5556/dex ATENEA_CP_OIDC_CLIENT_ID=atenea-control-plane
  export ATENEA_CP_OIDC_CLIENT_SECRET=dev-only-secret-not-for-production
  export ATENEA_CP_OIDC_REDIRECT_URL=http://$CP_IP:8080/auth/callback
  export ATENEA_CP_ISSUER_KEY_FILE=$LAB/issuer.key ATENEA_CP_CONFIG_VALIDITY_SECS=60
  export ATENEA_CP_DEV_INSECURE_COOKIES=true ATENEA_CP_LISTEN=0.0.0.0:8080
  export ATENEA_CP_WEB_DIR=$ROOT/apps/web/build/client
  ATENEA_CP_DATABASE_URL=$ADMIN_URL ATENEA_CP_APPLICATION_ROLE=atenea_app atenea-control-plane --migrate > $LAB/migrate.log 2>&1 || { cat $LAB/migrate.log; exit 1; }
  ATENEA_CP_DATABASE_URL=postgres://atenea_app:app-pass@postgres/atenea \
    setsid nohup atenea-control-plane > $LAB/cp.log 2>&1 &

  # Coordinator, relay, web falso y HAProxy en inet
  ATENEA_COORD_LISTEN="[::]:8081" ATENEA_COORD_STUN_BIND="::" ATENEA_COORD_ANCHORS="[\"$ROOT_PUB\"]" \
    ATENEA_COORD_RELAY_SIGNING_KEY_FILE=$LAB/relay-signing.hex \
    ATENEA_COORD_RELAYS='[{"name":"lab","udp":"198.51.100.10:443","tls":"198.51.100.10:443","tls_name":"relay.atenea.one"}]' \
    setsid nohup ip netns exec inet atenea-coordinator > $LAB/coord.log 2>&1 &
  ATENEA_RELAY_UDP_LISTEN=0.0.0.0:443 ATENEA_RELAY_TCP_LISTEN=127.0.0.1:8443 \
    ATENEA_RELAY_COORDINATOR_KEYS="[\"$RELAY_PUB\"]" \
    ATENEA_RELAY_TLS_CERT_FILE=$LAB/certs/relay.pem ATENEA_RELAY_TLS_KEY_FILE=$LAB/certs/relay.key \
    ATENEA_RELAY_PROXY_PROTOCOL=true ATENEA_RELAY_STATUS_SOCKET=$LAB/relay.sock \
    setsid nohup ip netns exec inet atenea-relay > $LAB/relay.log 2>&1 &
  setsid nohup ip netns exec inet python3 "$ROOT/tests/network-lab/nat/fake-web.py" 8082 > /dev/null 2>&1 &
  sleep 1
  start_haproxy 8443 8082 || exit 1
  for _ in $(seq 1 30); do python3 -c "import urllib.request;urllib.request.urlopen('http://$CP_IP:8080/healthz',timeout=2)" 2>/dev/null && break; sleep 1; done
  python3 -c "import urllib.request;urllib.request.urlopen('http://$CP_IP:8080/healthz',timeout=2)" && echo "listo root_anchor=$ROOT_PUB"
  ;;
routes-setup)
  # Espacios separados: no reutilizar agentes ni reglas de bloqueo del escenario del panel.
  for n in r c h; do
    ip netns add "$n" || exit 1
    ip -n "$n" link set lo up
  done
  _topo_host r 1 20
  _topo_host c 2 20
  ip link add pr-lan type veth peer name ph-lan
  ip link set pr-lan netns r
  ip link set ph-lan netns h
  ip -n r addr add 192.168.50.1/24 dev pr-lan
  ip -n h addr add 192.168.50.20/24 dev ph-lan
  ip -n r link set pr-lan up
  ip -n h link set ph-lan up
  # Puerta inexistente: h no puede devolver tráfico al túnel sin SNAT.
  ip -n h route add default via 192.168.50.254
  ip netns exec r sysctl -qw net.ipv4.ip_forward=1
  mkdir -p "$LAB/dns-bin"
  cat > "$LAB/dns-bin/resolvectl" <<'SH'
#!/bin/sh
printf '%s\n' "$*" >> "/tmp/lab/resolvectl-${ATENEA_LAB_DEVICE:?}.log"
SH
  chmod +x "$LAB/dns-bin/resolvectl"
  ;;
issuer-cert)
  # Cada spec firma su propia red; la CLI no sobrescribe certificados.
  rm -f "$LAB/issuer.cert"
  atenea issuer-cert --root $LAB/root.key --issuer-key $LAB/issuer.key --network-id "${1//-/}" --serial 1 --valid-days 30 --out $LAB/issuer.cert
  ;;
device-init)
  atenea device-init --dir $LAB/$1 > $LAB/$1.info && cat $LAB/$1.info
  ;;
enroll)
  atenea enroll --url "$2" --token "$3" --dir $LAB/$1 --name "host-$1" --insecure-http
  ;;
start-agent)
  n=$1
  advertise=()
  if [[ "$n" == r ]]; then advertise=(--advertise-route 192.168.50.0/24); fi
  atenea agent-config --join "$2" --dir $LAB/$n --out $LAB/$n.toml --state-dir $LAB/$n.state --config-file $LAB/$n.cfg --ipc-socket $LAB/$n.sock --insecure-http "${advertise[@]}" >/dev/null || exit 1
  {
    echo "interval_secs = 10"
    printf '\n[coordinator]\nurl = "http://198.51.100.10:8081"\ninsecure_http = true\nstun = ["198.51.100.10:3478", "198.51.100.10:3479"]\nrelay_ca_file = "%s/certs/ca.pem"\n' "$LAB"
  } >> $LAB/$n.toml
  if [[ "$n" == r || "$n" == c ]]; then
    export ATENEA_RESOLVECTL="$LAB/dns-bin/resolvectl" ATENEA_LAB_DEVICE=$n
  fi
  ip netns exec $n atenea up --config $LAB/$n.toml --log $LAB/$n.log
  ;;
field)
  python3 "$TOOLS" field "$LAB/$1.sock" "$2"
  ;;
ping)
  ip netns exec "$1" ping -c 1 -W 1 "$2" > /dev/null 2>&1
  ;;
transfer)
  ip netns exec b python3 "$TOOLS" tcp-server "$1" 9000 > $LAB/srv.json &
  S=$!; sleep 0.5
  ip netns exec a python3 "$TOOLS" tcp-client "$1" 9000 1000000 9 > $LAB/cli.json
  wait $S
  python3 - <<PY
import json
print("ok" if json.load(open("$LAB/srv.json"))["sha256"] == json.load(open("$LAB/cli.json"))["sha256"] else "fail")
PY
  ;;
subnet-transfer)
  ip netns exec h python3 - > "$LAB/subnet-srv.json" <<'PY' &
import hashlib, json, socket
with socket.socket() as server:
    server.setsockopt(socket.SOL_SOCKET, socket.SO_REUSEADDR, 1)
    server.bind(("192.168.50.20", 9000))
    server.listen(1)
    server.settimeout(120)
    connection, peer = server.accept()
    connection.settimeout(120)
    digest, size = hashlib.sha256(), 0
    with connection:
        while data := connection.recv(65536):
            digest.update(data)
            size += len(data)
    print(json.dumps({"bytes": size, "sha256": digest.hexdigest(), "source": peer[0]}))
PY
  S=$!; sleep 0.5
  ip netns exec c python3 "$TOOLS" tcp-client 192.168.50.20 9000 1000000 9 > "$LAB/subnet-cli.json"
  wait "$S"
  python3 - "$LAB" <<'PY'
import json, pathlib, sys
lab = pathlib.Path(sys.argv[1])
server = json.loads((lab / "subnet-srv.json").read_text())
client = json.loads((lab / "subnet-cli.json").read_text())
assert server["sha256"] == client["sha256"], (server, client)
assert server["bytes"] == 1000000, server
assert server["source"] == "192.168.50.1", server
print(json.dumps(server))
PY
  ;;
subnet-route)
  ip -n c route show 192.168.50.0/24
  ;;
dns-log)
  cat "$LAB/resolvectl-$1.log"
  ;;
block)
  case "$1" in p2p) net_block_p2p;; udp) net_block_udp;; none) net_unblock_all;; *) exit 2;; esac
  ;;
*)
  echo "orden desconocida: $cmd" >&2; exit 2
  ;;
esac
