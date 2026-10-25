FROM ghcr.io/getsops/sops:v3.13.3@sha256:857f5a151ac0b2bfc55c1e4e5581d66fb8e268e4d106b38e74191f3bac9d58ea AS sops
FROM debian:13@sha256:4c25275446ca371766b8b093c696782eabce06263fccacb8d5d813b3dc44b308
ENV container=docker
RUN apt-get update && apt-get install -y --no-install-recommends systemd systemd-sysv dbus iproute2 nftables passwd ca-certificates python3 ansible-core ansible-lint yamllint age openssh-client openssh-server sudo chrony unattended-upgrades docker.io docker-cli docker-compose openssl curl && rm -rf /var/lib/apt/lists/*
COPY --from=sops /usr/local/bin/sops /usr/local/bin/sops
COPY --from=ghcr.io/opentofu/opentofu:1.10.6@sha256:879a73a0ee45132abf1b26583e3ab3a1e5443157a13b460114578d13dbe40b6e /usr/local/bin/tofu /usr/local/bin/tofu
STOPSIGNAL SIGRTMIN+3
CMD ["/sbin/init"]
