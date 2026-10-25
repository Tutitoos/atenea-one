#!/usr/bin/env bash
# Laboratorio efímero: sin puertos de host, sin cambios en imágenes/volúmenes anteriores.
set -euo pipefail
cd "$(dirname "$0")/../.."
export OBS_ID="atenea-obs-$$"
NET="$OBS_ID"
cleanup() {
  local status=$?
  if [[ $status != 0 ]]; then
    for service in lab prometheus loki alloy grafana; do docker logs "$OBS_ID-$service" 2>&1 || true; done
  fi
  docker rm -f "$OBS_ID-lab" "$OBS_ID-postgres" "$OBS_ID-dex" "$OBS_ID-prometheus" "$OBS_ID-loki" "$OBS_ID-alloy" "$OBS_ID-grafana" >/dev/null 2>&1 || true
  docker network rm "$NET" >/dev/null 2>&1 || true
  docker volume rm "$OBS_ID-logs" >/dev/null 2>&1 || true
  return "$status"
}
trap cleanup EXIT
PROM=prom/prometheus@sha256:6927e0919a144aa7616fd0137d4816816d42f6b816de3af269ab065250859a62
LOKI=grafana/loki@sha256:58a6c186ce78ba04d58bfe2a927eff296ba733a430df09645d56cdc158f3ba08
GRAFANA=grafana/grafana@sha256:62d2b9d20a19714ebfe48d1bb405086081bc602aa053e28cf6d73c7537640dfb
ALLOY=grafana/alloy@sha256:ab04df3936e4d71d31b6f55e0c58a7e749091f59635dd8c2bc731ba1b6c93701
docker run --rm --entrypoint /bin/promtool -v "$PWD/deploy/observability:/etc/prometheus:ro" "$PROM" check config /etc/prometheus/prometheus.yaml
docker run --rm --entrypoint /bin/promtool -v "$PWD/deploy/observability:/etc/prometheus:ro" "$PROM" check rules /etc/prometheus/alerts.yaml
docker network create "$NET" >/dev/null
docker volume create "$OBS_ID-logs" >/dev/null
docker run -d --name "$OBS_ID-postgres" --network "$NET" --network-alias postgres -e POSTGRES_PASSWORD=pg -e POSTGRES_DB=atenea postgres:17 >/dev/null
docker run -d --name "$OBS_ID-dex" --network "$NET" --network-alias dex -v "$PWD/deploy/compose/dev/dex.yaml:/etc/dex/config.yaml:ro" ghcr.io/dexidp/dex:v2.41.1 dex serve /etc/dex/config.yaml >/dev/null
docker run -d --name "$OBS_ID-loki" --network "$NET" --network-alias loki -v "$PWD/deploy/observability/loki.yaml:/etc/loki/config.yaml:ro" "$LOKI" -config.file=/etc/loki/config.yaml >/dev/null
docker run -d --name "$OBS_ID-grafana" --network "$NET" --network-alias grafana -e GF_ANALYTICS_REPORTING_ENABLED=false -v "$PWD/deploy/observability/grafana/provisioning:/etc/grafana/provisioning:ro" -v "$PWD/deploy/observability/grafana/dashboards:/var/lib/grafana/dashboards:ro" "$GRAFANA" >/dev/null
docker run -d --name "$OBS_ID-alloy" --network "$NET" -v "$PWD/deploy/observability/alloy.alloy:/etc/alloy/config.alloy:ro" -v "$OBS_ID-logs:/var/lib/docker/containers:ro" "$ALLOY" run /etc/alloy/config.alloy >/dev/null
docker run -d --name "$OBS_ID-lab" --network "$NET" --network-alias lab --privileged -v "$PWD:/src:ro" -v atenea-lab-target:/cache -v atenea-cargo-registry:/usr/local/cargo/registry -v "$OBS_ID-logs:/obs/logs" -e CARGO_TARGET_DIR=/cache/target -e RUST_LOG=info -w /src atenea-lab bash tests/observability/lab.sh >/dev/null
# Prometheus usa configuración de laboratorio, pero el mismo scrape y reglas del overlay.
docker run -d --name "$OBS_ID-prometheus" --network "$NET" --network-alias prometheus -v "$PWD/tests/observability/prometheus.yaml:/etc/prometheus/prometheus.yaml:ro" -v "$PWD/deploy/observability/alerts.yaml:/etc/prometheus/alerts.yaml:ro" "$PROM" --config.file=/etc/prometheus/prometheus.yaml >/dev/null
docker exec "$OBS_ID-lab" python3 tests/observability/check.py
