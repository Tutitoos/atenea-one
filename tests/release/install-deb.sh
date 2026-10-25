#!/usr/bin/env bash
# El modo release de tests/packaging usa Debian/systemd precargado, sin apt.
set -euo pipefail
dist=$(cd "$1" && pwd)
commit=$2
name="atenea-release-install-$$"
trap 'docker rm -f "$name" >/dev/null 2>&1 || true' EXIT
docker run -d --pull never --network none --name "$name" --privileged --cgroupns=host \
    --tmpfs /run --tmpfs /run/lock --tmpfs /tmp \
    -v /sys/fs/cgroup:/sys/fs/cgroup:rw -v "$dist:/release:ro" \
    --entrypoint /sbin/init atenea-infra:local >/dev/null
for ((i=0; i<60; i++)); do
    if docker exec "$name" systemctl show --property=Version --value >/dev/null 2>&1; then break; fi
    sleep 1
done
docker exec -e EXPECTED_COMMIT="$commit" "$name" bash -c '
    set -euo pipefail
    # Esta base de infraestructura carece de resolved (Recommends), no se descarga.
    dpkg -i /release/atenea.deb
    [ "$(dpkg-query -W -f='"'"'${X-Atenea-Commit}'"'"' atenea)" = "$EXPECTED_COMMIT" ]
    version=$(python3 -c "import json; print(json.load(open(\"/release/release-manifest.json\"))[\"version\"])")
    [ "$(atenea --version)" = "atenea $version ($EXPECTED_COMMIT)" ]
    [ "$(atenea-agent --version)" = "atenea-agent $version ($EXPECTED_COMMIT)" ]
    systemd-analyze verify /usr/lib/systemd/system/atenea-agent.service
    ! systemctl is-active --quiet atenea-agent
    ! systemctl is-enabled --quiet atenea-agent
    echo "PASS .deb instalado en Debian con systemd; versión y commit del manifiesto"
'
