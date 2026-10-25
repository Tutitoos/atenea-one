#!/usr/bin/env bash
set -euo pipefail
cargo build --release --locked -p atenea-cli -p atenea-agent -p atenea-control-plane -p atenea-coordinator -p atenea-relay
export PATH="$CARGO_TARGET_DIR/release:$PATH"
export ATENEA_CP_METRICS_LISTEN=0.0.0.0:9101 ATENEA_COORD_METRICS_LISTEN=0.0.0.0:9102 ATENEA_RELAY_METRICS_LISTEN=0.0.0.0:9103
for attempt in $(seq 1 60); do pg_isready -h postgres -U postgres >/dev/null 2>&1 && break; sleep 1; done
bash tests/e2e-panel/lab.sh setup
python3 tests/observability/traffic.py
# Mantiene servicios y namespaces mientras Prometheus/Alloy consumen las muestras.
sleep infinity
