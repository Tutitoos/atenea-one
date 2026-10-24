#!/usr/bin/env bash
# Biblioteca para source dentro de un contenedor Linux privilegiado y desechable.
# El llamador debe instalar: trap topo_teardown EXIT INT TERM.

topo_teardown() {
  local n p iface
  for n in a b inet nat1 nat2 nat3; do
    for p in $(ip netns pids "$n" 2>/dev/null || true); do kill "$p" 2>/dev/null || true; done
    ip netns del "$n" 2>/dev/null || true
    for iface in eth0 wan lan; do
      ip link del "nl-$n-$iface" 2>/dev/null || true
    done
  done
  for n in nl-public nl-lan1 nl-lan2 nl-lan3; do
    ip link del "$n" 2>/dev/null || true
  done
  ip link del tmp-nl 2>/dev/null || true
}

_topo_bridge() {
  # Inundar multicast evita depender de la convergencia MLD para resolver ND.
  ip link add "$1" type bridge mcast_snooping 0
  ip link set "$1" up
}
_topo_attach() {
  local ns=$1 iface=$2 bridge=$3 host="nl-$1-$2"
  ip link add "$host" type veth peer name tmp-nl
  ip link set tmp-nl netns "$ns"
  ip -n "$ns" link set tmp-nl name "$iface"
  ip link set "$host" master "$bridge" up
  ip -n "$ns" link set "$iface" up
}
_topo_addr() { ip -n "$1" addr add "$3" dev "$2"; }
_topo_v6() { ip -n "$1" -6 addr add "$3" dev "$2" nodad; }
_topo_router() {
  local ns=$1 public=$2 lan=$3
  _topo_attach "$ns" wan nl-public
  _topo_addr "$ns" wan "$public/24"
  _topo_bridge "nl-lan$lan"
  _topo_attach "$ns" lan "nl-lan$lan"
  _topo_addr "$ns" lan "192.168.$lan.1/24"
  ip netns exec "$ns" sysctl -qw net.ipv4.ip_forward=1
  ip netns exec "$ns" nft -f - <<'NFT'
table inet firewall {
 # No confirmar flujos UDP WAN locales antes de que el cliente abra su salida.
 chain input { type filter hook input priority filter; policy accept;
  iifname "wan" meta l4proto udp ct state new drop
 }
 chain forward { type filter hook forward priority filter; policy drop;
  ct state established,related accept
  iifname "lan" oifname "wan" accept
 }
}
table ip nat {
 chain postrouting { type nat hook postrouting priority srcnat; policy accept;
  oifname "wan" masquerade
 }
}
NFT
}
_topo_host() {
  _topo_attach "$1" eth0 "nl-lan$2"
  _topo_addr "$1" eth0 "192.168.$2.$3/24"
  ip -n "$1" route replace default via "192.168.$2.1"
}

topo_setup() {
  local n topology=$1
  case "$topology" in flat|home|symmetric|double|samelan_hairpin_off|samelan_hairpin_on|ipv6) ;; *) echo "Topología desconocida: $topology" >&2; return 1;; esac
  topo_teardown
  TOPOLOGY=$topology
  for n in a b inet nat1 nat2 nat3; do
    ip netns add "$n"
    ip -n "$n" link set lo up
  done
  _topo_bridge nl-public
  _topo_attach inet eth0 nl-public
  _topo_addr inet eth0 198.51.100.10/24
  _topo_v6 inet eth0 2001:db8:100::10/64
  if [[ "$topology" == flat ]]; then
    for n in a b; do
      local suffix=11; [[ "$n" == b ]] && suffix=12
      _topo_attach "$n" eth0 nl-public
      _topo_addr "$n" eth0 "198.51.100.$suffix/24"
      _topo_v6 "$n" eth0 "2001:db8:100::$suffix/64"
    done
    return
  fi
  if [[ "$topology" == ipv6 ]]; then
    for n in a b; do
      local router=nat1 suffix=1; [[ "$n" == b ]] && router=nat2 && suffix=2
      _topo_bridge "nl-lan$suffix"
      _topo_attach "$router" wan nl-public
      _topo_attach "$router" lan "nl-lan$suffix"
      _topo_v6 "$router" wan "2001:db8:100::$suffix/64"
      _topo_v6 "$router" lan "2001:db8:$n::1/64"
      _topo_attach "$n" eth0 "nl-lan$suffix"
      _topo_v6 "$n" eth0 "2001:db8:$n::10/64"
      ip -n "$n" -6 route add default via "2001:db8:$n::1"
      ip netns exec "$router" sysctl -qw net.ipv6.conf.all.forwarding=1
      ip -n inet -6 route add "2001:db8:$n::/64" via "2001:db8:100::$suffix"
      ip netns exec "$router" nft -f - <<'NFT'
table inet firewall {
 chain forward { type filter hook forward priority filter; policy drop;
  ct state established,related accept
  iifname "lan" oifname "wan" accept
 }
}
NFT
    done
    ip -n nat1 -6 route add 2001:db8:b::/64 via 2001:db8:100::2
    ip -n nat2 -6 route add 2001:db8:a::/64 via 2001:db8:100::1
    return
  fi
  _topo_router nat1 198.51.100.1 1
  _topo_host a 1 10
  if [[ "$topology" == samelan_* ]]; then
    _topo_host b 1 11
    if [[ "$topology" == samelan_hairpin_on ]]; then
      # Puertos del laboratorio; el consumidor añade sus DNAT si usa otros.
      ip netns exec nat1 nft -f - <<'NFT'
add chain ip nat prerouting { type nat hook prerouting priority dstnat; policy accept; }
add rule ip nat prerouting iifname "lan" ip daddr 198.51.100.1 udp dport 42000 dnat to 192.168.1.10:42000
add rule ip nat prerouting iifname "lan" ip daddr 198.51.100.1 udp dport 42001 dnat to 192.168.1.11:42001
add rule ip nat postrouting iifname "lan" oifname "lan" masquerade
add rule inet firewall forward iifname "lan" oifname "lan" ct status dnat accept
NFT
    fi
    return
  fi
  _topo_router nat2 198.51.100.2 2
  _topo_host b 2 10
  if [[ "$topology" == symmetric ]]; then
    # Rangos disjuntos evitan que una coincidencia aleatoria falsee la sonda.
    ip netns exec nat1 nft flush chain ip nat postrouting
    ip netns exec nat1 nft -f - <<'NFT'
add rule ip nat postrouting oifname "wan" udp dport 3478 masquerade to :40000-44999 fully-random
add rule ip nat postrouting oifname "wan" udp dport 3479 masquerade to :45000-49999 fully-random
add rule ip nat postrouting oifname "wan" meta l4proto udp masquerade to :50000-59999 fully-random
add rule ip nat postrouting oifname "wan" masquerade
NFT
  elif [[ "$topology" == double ]]; then
    _topo_router nat3 198.51.100.3 3
    ip -n nat1 link del wan
    _topo_attach nat1 wan nl-lan3
    _topo_addr nat1 wan 100.64.0.2/10
    _topo_addr nat3 lan 100.64.0.1/10
    ip -n nat1 route add default via 100.64.0.1
  elif [[ "$topology" == home ]]; then
    _topo_router nat3 198.51.100.3 3
  fi
}

topo_move_a_to_cafe() {
  [[ "${TOPOLOGY:-}" == home ]] || return 1
  ip -n a link del eth0
  _topo_host a 3 10
}
topo_move_a_home() {
  [[ "${TOPOLOGY:-}" == home ]] || return 1
  ip -n a link del eth0
  _topo_host a 1 10
}

topo_describe() {
  cat <<'TEXT'
Todos: inet/eth0 = 198.51.100.10/24, 2001:db8:100::10/64.
flat: a/eth0 = 198.51.100.11, 2001:db8:100::11; b = .12, ::12.
home/symmetric: a/eth0 = 192.168.1.10/24 vía .1; b/eth0 = 192.168.2.10/24 vía .1.
nat1/wan = 198.51.100.1/24; nat2/wan = .2; nat3/wan = .3.
Cada natN/lan = 192.168.N.1/24. home café: a/eth0 = 192.168.3.10/24 vía .1.
double: nat1/wan = 100.64.0.2/10 vía 100.64.0.1 (nat3/lan); b como home.
samelan_*: a = 192.168.1.10/24; b = 192.168.1.11/24; gateway = 192.168.1.1.
Hairpin on: DNAT para UDP 42000 → a y 42001 → b; SNAT hacia la LAN.
ipv6: a/eth0 = 2001:db8:a::10/64 vía ::1; b = 2001:db8:b::10/64 vía ::1.
ipv6: nat1/wan = 2001:db8:100::1; nat2/wan = ::2; lan = 2001:db8:a::1 / b::1.
TEXT
}

# Tabla separada: antes de conntrack/DNAT y del accept established existente.
# En a/b cubre también LAN compartida y flat, que no atraviesan routers.
_net_block() {
  local mode=$1 ns rules
  case "$mode" in
    p2p) rules='ip saddr 198.51.100.10 return
      ip daddr 198.51.100.10 return
      ip6 saddr 2001:db8:100::10 return
      ip6 daddr 2001:db8:100::10 return
      ip saddr { 192.168.0.0/16, 100.64.0.0/10, 198.51.100.0/24 } ip daddr { 192.168.0.0/16, 100.64.0.0/10, 198.51.100.0/24 } counter drop
      ip6 saddr { 2001:db8::/32, fe80::/10 } ip6 daddr { 2001:db8::/32, fe80::/10 } counter drop';;
    udp) rules='counter drop';;
    relay_udp) rules='ip daddr 198.51.100.10 udp dport 443 counter drop
      ip6 daddr 2001:db8:100::10 udp dport 443 counter drop';;
    *) return 2;;
  esac
  for ns in a b nat1 nat2 nat3; do
    ip netns exec "$ns" nft list table inet relay_lab >/dev/null 2>&1 ||
      ip netns exec "$ns" nft add table inet relay_lab
    ip netns exec "$ns" nft -f - <<NFT
flush table inet relay_lab
add chain inet relay_lab udp_filter
add chain inet relay_lab early_input { type filter hook prerouting priority -310; policy accept; }
add chain inet relay_lab early_output { type filter hook output priority -310; policy accept; }
add rule inet relay_lab early_input meta l4proto udp jump udp_filter
add rule inet relay_lab early_output meta l4proto udp jump udp_filter
$(while IFS= read -r rule; do printf 'add rule inet relay_lab udp_filter %s\n' "$rule"; done <<<"$rules")
NFT
  done
}

# Cada bloqueador sustituye al anterior; no altera NAT ni vacía conntrack.
net_block_p2p() { _net_block p2p; }
net_block_udp() { _net_block udp; }
net_block_relay_udp() { _net_block relay_udp; }
net_unblock_all() {
  local ns
  for ns in a b nat1 nat2 nat3; do
    if ip netns exec "$ns" nft list table inet relay_lab >/dev/null 2>&1; then
      ip netns exec "$ns" nft delete table inet relay_lab
    fi
  done
}

start_haproxy() {
  local relay=$1 web=$2 template config
  for port in "$relay" "$web"; do
    [[ "$port" =~ ^[0-9]+$ ]] && ((port > 0 && port <= 65535)) || return 2
  done
  template="$(dirname "${BASH_SOURCE[0]}")/haproxy.cfg.tpl"
  config=$(mktemp /tmp/atenea-haproxy.XXXXXX)
  # HAProxy lee la configuración antes de daemonizar; no quedan ficheros temporales.
  if ! python3 - "$template" "$config" "$relay" "$web" <<'PY'
import pathlib, sys
template, target, relay, web = sys.argv[1:]
pathlib.Path(target).write_text(pathlib.Path(template).read_text()
                              .replace("@RELAY_PORT@", relay).replace("@WEB_PORT@", web))
PY
  then rm -f "$config"; return 1; fi
  local result=0
  ip netns exec inet haproxy -D -f "$config" || result=$?
  rm -f "$config"
  return "$result"
}
