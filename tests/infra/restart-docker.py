#!/usr/bin/env python3
"""Tres reinicios reales del motor: diagnóstico previo o recuperación automática."""
import json
import os
import subprocess
import sys
import time

stage = sys.argv[1] if len(sys.argv) == 2 else 'after'
assert stage in ('before', 'after')
compose = ['docker', 'compose', '--project-directory', '/opt/atenea']
expected = {'netns', 'haproxy', 'relay', 'caddy', 'control-plane', 'coordinator', 'dex', 'postgres'}


def snapshot(probes=True):
    ids = subprocess.check_output(compose + ['ps', '--all', '-q'], text=True).split()
    containers = json.loads(subprocess.check_output(['docker', 'inspect', *ids], text=True))
    result = {}
    for container in containers:
        name = container['Config']['Labels']['com.docker.compose.service']
        state = container['State']
        pid = state['Pid']
        result[name] = {'state': state['Status'], 'health': state.get('Health', {}).get('Status'),
                       'netns': os.readlink(f'/proc/{pid}/ns/net') if pid else None}
        if probes and name in ('haproxy', 'relay'):
            check = container['Config']['Healthcheck']['Test']
            assert check[0] in ('CMD', 'CMD-SHELL')
            args = check[1:] if check[0] == 'CMD' else ['/bin/sh', '-c', check[1]]
            probe = subprocess.run(['docker', 'exec', container['Id'], *args], text=True, capture_output=True)
            result[name]['probe_exit'] = probe.returncode
            result[name]['probe_output'] = (probe.stdout + probe.stderr)[-1500:]
    return result


def assert_healthy(state):
    assert set(state) == expected, state
    assert all(s['health'] == 'healthy' for s in state.values()), state
    assert all(state[n]['netns'] == state['netns']['netns'] for n in ('haproxy', 'relay')), state
    assert all(state[n]['probe_exit'] == 0 for n in ('haproxy', 'relay')), state


if stage == 'after':
    assert_healthy(snapshot())
    began = time.time()
    owner = subprocess.check_output(compose + ['ps', '-q', 'netns'], text=True).strip()
    subprocess.run(['docker', 'restart', owner], check=True)
    deadline = time.time() + 180
    while True:
        state = snapshot(probes=False)
        if all(state[n]['health'] == 'unhealthy' for n in ('haproxy', 'relay')):
            break
        assert time.time() < deadline, state
        time.sleep(1)
    state = snapshot()
    assert all(state[n]['probe_exit'] != 0 for n in ('haproxy', 'relay')), state
    print('STALE-NAMESPACE ' + json.dumps({'elapsed': round(time.time()-began, 3), 'services': state}), flush=True)
    subprocess.run(['systemctl', 'restart', 'atenea-stack.service'], check=True)
    assert_healthy(snapshot())
    print('PASS namespace obsoleto: probes fallan; unidad reconstruye entrada', flush=True)

for repetition in range(1, 4):
    subprocess.run(['docker', 'ps', '-a'], check=True)
    subprocess.run(['docker', 'network', 'ls'], check=True)
    began = time.time()
    subprocess.run(['systemctl', 'restart', 'docker'], check=True)
    if stage == 'before':
        # Ventana fija para observar procesos/namespace y ejecutar sus probes, sin curarlos.
        time.sleep(12)
        state = snapshot()
        print('DOCKER-RESTART ' + json.dumps({'stage': stage, 'repetition': repetition,
              'began': began, 'elapsed': round(time.time()-began, 3), 'services': state}), flush=True)
        # Restablecer exactamente el punto de partida entre las tres reproducciones.
        subprocess.run(compose + ['up', '-d', '--force-recreate', '--wait', '--wait-timeout', '180'], check=True)
        assert_healthy(snapshot())
    else:
        # Espera el job ya arrancado por docker.service; no repara contenedores desde la prueba.
        subprocess.run(['systemctl', 'start', 'atenea-stack.service'], check=True)
        state = snapshot()
        assert_healthy(state)
        print('DOCKER-RESTART ' + json.dumps({'stage': stage, 'repetition': repetition,
              'began': began, 'elapsed': round(time.time()-began, 3), 'services': state}), flush=True)
print(f'PASS Docker restart {stage}: 3 repeticiones reales', flush=True)
