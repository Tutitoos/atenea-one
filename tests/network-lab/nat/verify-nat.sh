#!/usr/bin/env bash
# Copyright 2026 Tutitoos
# SPDX-License-Identifier: Apache-2.0
# No compila ni ejecuta software de Atenea: solo namespaces, nftables y UDP.
set -Eeuo pipefail
cd "$(dirname "$0")/../../.."
source tests/network-lab/nat/lib.sh
PROBE=$PWD/tests/network-lab/nat/natprobe.py
WORK=$(mktemp -d /tmp/nat-lab.XXXXXX)
cleanup() { topo_teardown; rm -rf "$WORK"; }
trap cleanup EXIT
trap 'exit 130' INT
trap 'exit 143' TERM
trap 'echo "FAIL línea $LINENO: $BASH_COMMAND" >&2' ERR
echo "Kernel: $(uname -sr)"
nft --version
python3 --version
FAILS=0
assert_json() {
  local label=$1 data=$2 expression=$3
  echo "  $label: $data"
  if python3 -c 'import json,sys; d=json.loads(sys.argv[1]); sys.exit(0 if eval(sys.argv[2], {"__builtins__": {}}, {"d": d}) else 1)' "$data" "$expression"; then
    echo "  OK   $label"
  else
    echo "  FAIL $label"; FAILS=$((FAILS + 1))
  fi
}
start_reflector() {
  ip netns exec inet python3 -u "$PROBE" reflect >"$WORK/reflector" &
  local pid=$! tries
  for tries in {1..100}; do
    if [[ -s "$WORK/reflector" ]]; then return; fi
    if ! kill -0 "$pid" 2>/dev/null; then echo 'FAIL reflector terminado' >&2; exit 1; fi
    sleep 0.02
  done
  echo 'FAIL reflector no disponible' >&2; exit 1
}
probe_mapping() { ip netns exec "$1" python3 "$PROBE" mapping --ip "$2"; }

for topology in flat home symmetric double samelan_hairpin_off samelan_hairpin_on ipv6; do
  echo "== $topology =="
  topo_setup "$topology"
  : >"$WORK/reflector"
  start_reflector
  server=198.51.100.10
  [[ "$topology" == ipv6 ]] && server=2001:db8:100::10
  expected=independent
  [[ "$topology" == symmetric ]] && expected=dependent
  a_mapping=$(probe_mapping a "$server")
  assert_json 'mapeo a (mismo puerto local 41000)' "$a_mapping" "d['mapping'] == '$expected'"
  assert_json 'mapeo b (mismo puerto local 41000)' "$(probe_mapping b "$server")" "d['mapping'] == 'independent'"
  if [[ "$topology" == flat ]]; then
    assert_json 'a sin NAT' "$a_mapping" "d['endpoints'] == [['198.51.100.11', 41000], ['198.51.100.11', 41000]]"
    assert_json 'entrada IPv6 permitida (control positivo del receptor)' \
      "$(python3 "$PROBE" unsolicited --ip 2001:db8:100::12)" "d['blocked'] == False"
  elif [[ "$topology" == ipv6 ]]; then
    assert_json 'IPv6 sin NAT' "$a_mapping" "d['endpoints'] == [['2001:db8:a::10', 41000], ['2001:db8:a::10', 41000]]"
    assert_json 'entrada IPv6 nueva sin salida previa denegada' \
      "$(python3 "$PROBE" unsolicited --ip 2001:db8:b::10)" "d['blocked'] == True"
  fi
  success=True
  [[ "$topology" == symmetric || "$topology" == samelan_hairpin_off ]] && success=False
  pair=$(python3 "$PROBE" pair --ip "$server")
  assert_json 'punch / hairpin (puertos locales 42000 y 42001)' "$pair" "d['success'] == $success and d['received'] == [$success, $success]"
  if [[ "$topology" == home ]]; then
    ns_before=$(ip netns exec a readlink /proc/self/ns/net)
    topo_move_a_to_cafe
    cafe=$(probe_mapping a "$server")
    assert_json 'café: nuevo endpoint y conectividad' "$cafe" "d['mapping'] == 'independent' and d['endpoints'][0] == ['198.51.100.3', 41000]"
    ns_after=$(ip netns exec a readlink /proc/self/ns/net)
    if [[ "$ns_before" == "$ns_after" ]]; then echo '  OK   mismo namespace a tras cambio'; else echo '  FAIL namespace a reemplazado'; FAILS=$((FAILS + 1)); fi
    topo_move_a_home
    assert_json 'retorno a casa' "$(probe_mapping a "$server")" "d['endpoints'][0] == ['198.51.100.1', 41000]"
  fi
  topo_teardown
  # La limpieza se comprueba sin confiar solo en el código de salida de ip del.
  if [[ -n "$(ip netns list)" ]]; then echo 'FAIL namespaces residuales'; exit 1; fi
  for bridge in nl-public nl-lan1 nl-lan2 nl-lan3; do
    if ip link show "$bridge" >/dev/null 2>&1; then echo "FAIL puente residual: $bridge"; exit 1; fi
  done
done
printf '\nResultado: %s fallos\n' "$FAILS"
[[ "$FAILS" == 0 ]]
