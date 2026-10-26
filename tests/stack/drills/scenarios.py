# Copyright 2026 Tutitoos
# SPDX-License-Identifier: Apache-2.0
"""Invocado por api.py tras OIDC, alta y arranque reales. Nunca imprime secretos."""
import os
import sys
import threading
os.environ['PATH'] = '/cache/target/release:' + os.environ['PATH']

# api.py aporta api, lab, wait_path, net, devices, ip_b y el cliente HTTPS autenticado.
def emit(kind, **values):
    print(kind + ' ' + json.dumps(values, sort_keys=True), flush=True)


def action(verb, *services):
    emit('ACTION', verb=verb, services=services)
    reply = json.loads(sys.stdin.readline())
    assert reply['ok'], reply
    return reply


def status(name='a'):
    return json.loads(subprocess.check_output(['atenea', 'status', '--socket', '/tmp/lab/'+name+'.sock'], text=True))

def peer_b(state):
    ident = next(d['id'].replace('-', '') for d in devices if d['name'] == 'host-b')
    return next((p for p in state['peers'] if bytes(p['device_id']).hex() == ident), {'path':'none'})


def wait_path(expected):
    until(lambda: peer_b(status())['path'] == expected and healthy())



def until(predicate, seconds=180):
    deadline = time.time() + seconds
    while time.time() < deadline:
        if predicate():
            return time.time()
        time.sleep(.2)
    raise AssertionError('Tiempo agotado esperando condición')


samples = []
finished = threading.Event()

def traffic():
    while not finished.is_set():
        began = time.time()
        result = subprocess.run(['ip', 'netns', 'exec', 'a', 'ping', '-n', '-c', '1', '-W', '1', ip_b], stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
        sample = {'sent': began, 'done': time.time(), 'ok': result.returncode == 0}
        samples.append(sample)
        emit('PING', **sample)
        finished.wait(.2)


threading.Thread(target=traffic, daemon=True).start()

def healthy():
    return samples and samples[-1]['ok'] and time.time()-samples[-1]['done'] < 2

def healthy_since(wall):
    return healthy() and samples[-1]['sent'] >= wall


def summary(label, began, ended, **extra):
    window = [s for s in samples if began <= s['sent'] <= ended]
    emit('RESULT', scenario=label, began=began, ended=ended,
         elapsed=round(ended-began, 3), probes=len(window), failed=sum(not s['ok'] for s in window), **extra)


def path(mode):
    # El cliente conserva un TLS sano; liberar por directo antes de exigir otra asignación UDP.
    if mode == 'relay-udp' and peer_b(status())['path'] == 'relay-tls':
        lab('block', 'none')
        wait_path('direct')
        until(lambda: relay_name() is None, 45)
    lab('block', {'direct':'none', 'relay-udp':'p2p', 'relay-tls':'udp'}[mode])
    wait_path(mode)
    assert lab('transfer', ip_b) == 'ok'
    emit('TRANSFER', wall=time.time(), path=mode, bytes=1000000, sha256='coincidente')


def relay_name():
    ident = next(d['id'].replace('-', '') for d in devices if d['name'] == 'host-b')
    entries = [e for e in status()['relay']['entries'] if e['peer'] == ident]
    return entries[0]['relay'] if entries else None


def panel_available():
    try:
        request('/healthz')
        return True
    except (urllib.error.URLError, TimeoutError):
        return False


try:
    path('direct')
    if os.environ['DRILL_PROFILE'] == 'lab':
        for repetition in range(1, 3):
            path('relay-udp')
            emit('BASELINE', relay=relay_name(), repetition=repetition)
            stopped = action('stop', 'relay')
            t = stopped['began']
            until(lambda: relay_name() == 'secundario' and healthy(), 100)
            summary('relay-failover', t, time.time(), repetition=repetition, relay=relay_name())
            action('start', 'relay')
            # Una nueva asignación vuelve al orden principal; no se exige failback automático.
            path('direct')
            time.sleep(32)

        for mode in ('direct', 'relay-udp'):
            for repetition in range(1, 3):
                path(mode)
                # Nuevo dispositivo sin endpoint firmado ni sesión previa: necesita rendezvous real.
                name = 'c'+str(len(joins))
                subprocess.run(['bash', '-c', 'source /src/tests/network-lab/nat/lib.sh; ip netns add "$1"; ip -n "$1" link set lo up; _topo_host "$1" 1 "$2"', '_', name, str(20+len(joins))], check=True)
                lab('device-init', name)
                enrollment = api(net+'/enrollment-tokens', {'ttl_secs':3600})
                joins[name] = json.dumps(enrollment['join'])
                lab('enroll', name, enrollment['join']['url'], enrollment['token'])
                device = next(d for d in api(net+'/devices') if d['name'] == 'host-'+name)
                api(net+'/devices/'+device['id']+'/approve', b'')
                ip_new = next(d['ipv4'] for d in api(net+'/devices') if d['id'] == device['id'])
                stopped = action('stop', 'coordinator')
                lab('start-agent', name, joins[name])
                assert not lab('ping', 'a', ip_new, check=False), 'Rendezvous nuevo durante caída'
                time.sleep(8)
                summary('coordinator-'+mode, stopped['began'], time.time(), repetition=repetition, new_rendezvous=False)
                back = action('start', 'coordinator')
                until(lambda: all((status(n)['coordinator']['last_announce_ok_unix'] or 0) >= back['ended'] for n in ('a','b')), 45)
                summary('coordinator-presence-'+mode, back['ended'], time.time(), repetition=repetition)
                until(lambda: lab('ping', 'a', ip_new, check=False), 150)
                summary('coordinator-new-peer-'+mode, back['ended'], time.time(), repetition=repetition,
                        last_announce=status()['coordinator']['last_announce_ok_unix'])
                api(net+'/devices/'+device['id']+'/revoke', b'')
                for pid in subprocess.check_output(['ip', 'netns', 'pids', name], text=True).split():
                    os.kill(int(pid), 15)
                subprocess.run(['ip', 'netns', 'del', name], check=True)
                # Cada repetición parte de los mismos dos agentes, sin acumular pares ociosos.
                until(lambda: all(len(status(n)['peers']) == 1 for n in ('a','b')), 60)

        for mode in ('direct', 'relay-udp', 'relay-tls'):
            for repetition in range(1, 3):
                path(mode)
                stopped = action('stop', 'haproxy')
                assert not panel_available()
                time.sleep(8)
                summary('haproxy-'+mode, stopped['began'], time.time(), repetition=repetition, panel=False)
                back = action('start', 'haproxy')
                until(lambda: panel_available() and healthy_since(back['ended']), 150)
                summary('haproxy-recovery-'+mode, back['ended'], time.time(), repetition=repetition)

        path('relay-udp')
        stopped = action('stop', 'relay', 'coordinator')
        until(lambda: relay_name() == 'secundario' and healthy(), 100)
        summary('relay+coordinator', stopped['began'], time.time(), relay=relay_name())
        action('start', 'relay', 'coordinator')

    for service in ('control-plane', 'postgres'):
        for repetition in range(1, 3):
            lease_path = 'direct' if repetition == 1 else 'relay-udp'
            path(lease_path)
            # Esperar una renovación fresca para medir también la validez nominal completa.
            previous = status()['config']['valid_until_unix']
            until(lambda: status()['config']['valid_until_unix'] > previous, 100)
            stopped = action('stop', service)
            leases = {n:status(n)['config']['valid_until_unix'] for n in ('a','b')}
            expiry = min(leases.values())
            device_b = next(d for d in devices if d['name']=='host-b')
            try:
                api(net+'/devices/'+device_b['id']+'/revoke', b'')
                raise AssertionError('Revocación aceptada con emisor/BD caído')
            except (urllib.error.URLError, TimeoutError) as error:
                emit('REVOCATION-UNAVAILABLE', service=service, error=type(error).__name__, wall=time.time())
            until(lambda: time.time() >= expiry+34, int(os.environ['DRILL_LEASE'])+50)
            assert not healthy(), 'No fue fail-closed'
            window = [s for s in samples if stopped['ended'] <= s['sent'] <= time.time()]
            last_ok = max(s['done'] for s in window if s['ok'])
            first_failed = min(s['sent'] for s in window if not s['ok'] and s['sent'] > last_ok)
            assert last_ok < expiry+31 and first_failed >= expiry+28, (expiry,last_ok,first_failed)
            summary('lease-'+service, stopped['began'], time.time(), profile=os.environ['DRILL_PROFILE'], repetition=repetition,
                    valid_until=leases, last_ok=last_ok, first_failed=first_failed, path=lease_path,
                    cutoff_after_valid_until=[round(last_ok-expiry,3),round(first_failed-expiry,3)])
            back = action('start', service)
            until(lambda: healthy_since(back['ended']), 180)
            assert next(d for d in api(net+'/devices') if d['id']==device_b['id'])['status']=='approved', 'Revocación fallida no debe encolarse'
            summary('lease-recovery-'+service, back['ended'], time.time(), profile=os.environ['DRILL_PROFILE'], repetition=repetition)

    device_b = next(d for d in devices if d['name']=='host-b')
    t = time.time()
    api(net+'/devices/'+device_b['id']+'/revoke', b'')
    until(lambda: not healthy(), 150)
    summary('revocation-after-recovery', t, time.time())
    emit('COMPLETE', profile=os.environ['DRILL_PROFILE'], wall=time.time())
except Exception:
    for name in ('a', 'b'):
        emit('DIAGNOSTIC', agent=name, status=status(name))
    raise
finally:
    finished.set()
