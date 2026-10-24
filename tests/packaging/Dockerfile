FROM debian:13
ENV container=docker
RUN apt-get update && apt-get install -y --no-install-recommends systemd systemd-sysv systemd-resolved dbus iproute2 nftables passwd ca-certificates libssl3t64 python3 shellcheck && rm -rf /var/lib/apt/lists/*
STOPSIGNAL SIGRTMIN+3
CMD ["/sbin/init"]
