FROM ghcr.io/getsops/sops:v3.11.0@sha256:6ce00ac946e1a52b2d1e5b7087441f7b6ef7bad907753104013f8bdb8f5fd891 AS sops
FROM debian:13@sha256:2dc419c6d7fe3dad0a2f9fd469c34cc90b0f9ba642d8fde837fe3d7d34a0e7f1
ENV container=docker
RUN apt-get update && apt-get install -y --no-install-recommends systemd systemd-sysv dbus iproute2 nftables passwd ca-certificates python3 ansible-core ansible-lint yamllint age openssh-client openssh-server sudo chrony unattended-upgrades docker.io docker-cli docker-compose openssl curl && rm -rf /var/lib/apt/lists/*
COPY --from=sops /usr/local/bin/sops /usr/local/bin/sops
COPY --from=ghcr.io/opentofu/opentofu:1.10.6@sha256:879a73a0ee45132abf1b26583e3ab3a1e5443157a13b460114578d13dbe40b6e /usr/local/bin/tofu /usr/local/bin/tofu
STOPSIGNAL SIGRTMIN+3
CMD ["/sbin/init"]
