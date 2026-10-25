#!/usr/bin/env bash
# Reutiliza las operaciones de fase6; los servidores aquí son exclusivamente Compose.
set -euo pipefail
export LAB_ROOT=/src
export PATH=/cache/target/release:$PATH
source /src/tests/network-lab/nat/lib.sh
case "${1:-}" in
setup)
  mkdir -p /tmp/lab
  cp /runtime/offline/root.key /tmp/lab/root.key
  cp /runtime/config/root.pub /tmp/lab/root.pub
  cp /runtime/secrets/issuer.key /tmp/lab/issuer.key
  mkdir -p /tmp/lab/certs
  cp /runtime/certs/ca.pem /tmp/lab/certs/ca.pem
  topo_setup home
  ip addr add 198.51.100.254/24 dev nl-public
  sysctl -qw net.ipv4.ip_forward=1
  for n in nat1 nat2 nat3 inet; do ip -n "$n" route add default via 198.51.100.254; done
  nft -f - <<NFT
table ip stack_egress {
 chain postrouting { type nat hook postrouting priority srcnat; oifname "eth0" ip saddr 198.51.100.0/24 ip daddr $CP_IP masquerade; }
}
NFT
  ;;
enroll)
  # La CA interna no forma parte del trust store público del CLI; canal interno aislado del ensayo.
  atenea enroll --url "http://$CP_IP:8080" --token "$4" --dir "/tmp/lab/$2" --name "host-$2"
  ;;
start-agent)
  n=$2
  join=$(python3 -c 'import json,sys; x=json.loads(sys.argv[1]);x["url"]="http://"+sys.argv[2]+":8080";print(json.dumps(x))' "$3" "$CP_IP")
  atenea agent-config --join "$join" --dir "/tmp/lab/$n" --out "/tmp/lab/$n.toml" --state-dir "/tmp/lab/$n.state" --config-file "/tmp/lab/$n.cfg" --ipc-socket "/tmp/lab/$n.sock" >/dev/null
  cat >> "/tmp/lab/$n.toml" <<TOML
interval_secs = 10
[coordinator]
url = "http://$COORD_IP:8081"
stun = ["$COORD_IP:3478", "$COORD_IP:3479"]
relay_ca_file = "/tmp/lab/certs/ca.pem"
TOML
  ip netns exec "$n" atenea up --config "/tmp/lab/$n.toml" --log "/tmp/lab/$n.log"
  ;;
*) exec bash /src/tests/e2e-panel/lab.sh "$@" ;;
esac
