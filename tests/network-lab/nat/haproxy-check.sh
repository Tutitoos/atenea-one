#!/usr/bin/env bash
# Copyright 2026 Tutitoos
# SPDX-License-Identifier: Apache-2.0
# Autónomo: topología plana dual-stack y servicios efímeros.
set -Eeuo pipefail
cd "$(dirname "$0")/../../.."
source tests/network-lab/nat/lib.sh
work=$(mktemp -d /tmp/haproxy-check.XXXXXX)
cleanup() { topo_teardown; rm -rf "$work"; }
trap cleanup EXIT
trap 'exit 130' INT
trap 'exit 143' TERM
topo_setup flat
bash tests/network-lab/nat/gen-certs.sh "$work/certs"
openssl verify -CAfile "$work/certs/ca.pem" -verify_hostname relay.atenea.one "$work/certs/relay.pem"
for address in 198.51.100.10 2001:db8:100::10; do
  openssl verify -CAfile "$work/certs/ca.pem" -verify_ip "$address" "$work/certs/relay.pem"
done
for file in ca.pem relay.pem relay.key; do [[ $(stat -c %a "$work/certs/$file") == 600 ]]; done
ip netns exec inet python3 tests/network-lab/nat/haproxy-probe.py serve 8443 \
  "$work/certs/relay.pem" "$work/certs/relay.key" >"$work/relay.log" 2>&1 &
ip netns exec inet python3 tests/network-lab/nat/fake-web.py 8081 >"$work/web.log" 2>&1 &
for attempt in {1..100}; do
  if [[ -s "$work/relay.log" && -s "$work/web.log" ]]; then break; fi
  sleep 0.02
done
start_haproxy 8443 8081
ip netns exec a python3 tests/network-lab/nat/haproxy-probe.py check \
  198.51.100.10 "$work/certs/ca.pem" 198.51.100.11
ip netns exec a python3 tests/network-lab/nat/haproxy-probe.py check \
  2001:db8:100::10 "$work/certs/ca.pem" 2001:db8:100::11
# Los errores de los hilos del backend también hacen fallar la comprobación.
[[ $(cat "$work/relay.log") == READY ]]
[[ $(cat "$work/web.log") == READY ]]
echo 'OK HAProxy IPv4/IPv6, certificado/SAN/permisos, sin errores de backend'
