#!/usr/bin/env python3
"""Aceptación contra Prometheus/Loki/Grafana reales; falla ante muestras ausentes."""
import json
import pathlib
import time
import urllib.parse
import urllib.request


def get(url):
    return json.load(urllib.request.urlopen(url, timeout=5))


def query(expression):
    return get('http://prometheus:9090/api/v1/query?'+urllib.parse.urlencode({'query':expression}))['data']['result']


for attempt in range(900):
    if pathlib.Path('/tmp/lab/observability.ready').exists():
        break
    time.sleep(1)
else:
    raise AssertionError('el laboratorio no completó tráfico y privacidad')
checks = {
    'tres scrapes sanos':'sum(up) == 3',
    'enrolamiento':'sum(atenea_http_requests_total{job="control-plane",route="/api/v1/enroll",code="201"}) > 0',
    'leases':'atenea_leases_issued_total{job="control-plane"} >= 2',
    'announce':'sum(atenea_http_requests_total{job="coordinator",route="/v1/announce",code="200"}) > 0',
    'presencias':'atenea_presences >= 2',
    'tokens relay':'atenea_relay_tokens_issued_total{job="coordinator"} >= 2',
    'rechazos relay':'sum(atenea_relay_token_rejections_total{job="coordinator"}) > 0',
    'sesiones UDP':'atenea_relay_udp_sessions >= 2',
    'tráfico relay':'sum(atenea_relay_packets_total{direction="egress"}) > 0',
    'histograma':'sum(atenea_http_duration_seconds_count) > 0',
}
for name, expression in checks.items():
    for attempt in range(60):
        if query(expression):
            print('PROMQL OK:',name)
            break
        time.sleep(1)
    else:
        raise AssertionError((name,expression))
private = json.loads(pathlib.Path('/tmp/lab/private.json').read_text())
for service in ['control-plane','coordinator','relay']:
    expression = '{service="'+service+'"}'
    for attempt in range(90):
        data = get('http://loki:3100/loki/api/v1/query_range?'+urllib.parse.urlencode({'query':expression,'limit':100}))
        rows = data['data']['result']
        if rows:
            raw = json.dumps(rows)
            assert all(secret not in raw for secret in private), 'fuga en Loki'
            assert all(set(row['stream']) == {'service'} for row in rows), rows
            print('LOKI OK:',service,'sin secretos ni etiquetas dinámicas')
            break
        time.sleep(1)
    else:
        raise AssertionError('Loki sin registros de '+service)
assert get('http://grafana:3000/api/health')['database'] == 'ok'
print('GRAFANA OK: salud de base de datos')
print('OBSERVABILIDAD: TODO OK')
