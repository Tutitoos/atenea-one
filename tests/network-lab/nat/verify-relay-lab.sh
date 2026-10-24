#!/usr/bin/env bash
# Solo herramientas de red: no requiere compilar Rust ni agentes.
set -Eeuo pipefail
cd "$(dirname "$0")/../../.."
source tests/network-lab/nat/lib.sh
work=$(mktemp -d /tmp/relay-lab.XXXXXX)
cleanup() { topo_teardown; rm -rf "$work"; }
trap cleanup EXIT
trap 'exit 130' INT
trap 'exit 143' TERM
trap 'echo "FAIL línea $LINENO: $BASH_COMMAND" >&2' ERR
echo "Kernel: $(uname -sr)"
nft --version
haproxy -v
python3 --version
started=$SECONDS
for topology in home symmetric double flat samelan_hairpin_off samelan_hairpin_on ipv6; do
  echo "== filtros relay: $topology =="
  topo_setup "$topology"
  host=198.51.100.10
  [[ "$topology" != ipv6 ]] || host=2001:db8:100::10
  ip netns exec inet python3 tests/network-lab/nat/relay-netprobe.py serve "$host" >"$work/server" 2>&1 &
  for attempt in {1..100}; do
    [[ ! -s "$work/server" ]] || break
    sleep 0.02
  done
  python3 tests/network-lab/nat/relay-netprobe.py check "$host" "$topology"
  for namespace in a b nat1 nat2 nat3; do
    if ip netns exec "$namespace" nft list table inet relay_lab >/dev/null 2>&1; then
      echo "FAIL tabla residual en $namespace" >&2; exit 1
    fi
  done
  topo_teardown
  [[ -z $(ip netns list) ]]
  for bridge in nl-public nl-lan1 nl-lan2 nl-lan3; do
    if ip link show "$bridge" >/dev/null 2>&1; then echo "FAIL puente residual: $bridge"; exit 1; fi
  done
  : >"$work/server"
done
bash tests/network-lab/nat/haproxy-check.sh
[[ -z $(ip netns list) ]]
echo "Resultado relay: 0 fallos; duración $((SECONDS - started))s"
