FROM rust:1.99-bookworm
RUN apt-get update \
 && apt-get install -y --no-install-recommends iproute2 iputils-ping tcpdump python3 postgresql-client nftables openssl haproxy \
 && rm -rf /var/lib/apt/lists/*
