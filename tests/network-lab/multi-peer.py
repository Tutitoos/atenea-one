"""Laboratorio multipar real; se ejecuta dentro de atenea-lab, sin compilación ni red externa."""
import itertools
import json
import os
from pathlib import Path
import socket
import subprocess
import sys
import time
import urllib.request

N = int(sys.argv[1])
BLOCK = sys.argv[2] == 'blocked'
LAB = Path('/tmp/lab')
LAB.mkdir()
os.environ['PATH'] = '/cache/target/release:' + os.environ['PATH']
names = ['a', 'b'] + ['c' + str(i) for i in range(2, N)]
processes = []


def run(*args, **kw):
    return subprocess.check_output(args, text=True, **kw).strip()


def shell(text):
    return run('bash', '-euc', 'source tests/network-lab/nat/lib.sh; ' + text)


def emit(kind, **data):
    print(kind + ' ' + json.dumps(data, sort_keys=True), flush=True)


def status(name):
    return json.loads(run('atenea', 'status', '--socket', str(LAB / (name + '.sock'))))


def spawn(ns, binary, log, **env):
    p = subprocess.Popen(['ip', 'netns', 'exec', ns, binary], env=os.environ | env,
                         stdout=open(LAB / log, 'w'), stderr=subprocess.STDOUT)
    processes.append(p)
    return p


def relay_status():
    with socket.socket(socket.AF_UNIX) as s:
        s.connect('/tmp/lab/relay.sock')
        return json.loads(s.makefile().readline())


try:
    shell('topo_setup home')
    for i, name in enumerate(names[2:], 20):
        shell(f'ip netns add {name}; ip -n {name} link set lo up; _topo_host {name} 1 {i}')
    if BLOCK:
        shell('net_block_p2p')
        rules = run('ip', 'netns', 'exec', 'a', 'nft', 'list', 'table', 'inet', 'relay_lab')
        for name in names[2:]:
            run('ip', 'netns', 'exec', name, 'nft', '-f', '-', input=rules)
    net = '00112233445566778899aabbccddeeff'
    root = run('atenea', 'root-init', '--out', '/tmp/lab/root.key')
    run('atenea', 'issuer-init', '--root', '/tmp/lab/root.key', '--network-id', net,
        '--serial', '1', '--valid-days', '30', '--out-dir', '/tmp/lab/issuer')
    ids = {}
    for name in names:
        info = run('atenea', 'device-init', '--dir', '/tmp/lab/' + name)
        ids[name] = dict(line.split('=', 1) for line in info.splitlines())['device_id']
    run('bash', 'tests/network-lab/nat/gen-certs.sh', '/tmp/lab/certs')
    (LAB / 'signing').write_text(os.urandom(32).hex())
    (LAB / 'signing').chmod(0o600)
    relay_id = os.urandom(16).hex()
    assert int(relay_id, 16) != 0
    (LAB / 'relay-id').write_text(relay_id)
    os.environ['ATENEA_COORD_RELAY_SIGNING_KEY_FILE'] = '/tmp/lab/signing'
    pub = run('atenea-coordinator', '--print-relay-key')
    spawn('inet', 'atenea-coordinator', 'coord.log', ATENEA_COORD_LISTEN='0.0.0.0:8081',
          ATENEA_COORD_STUN_BIND='0.0.0.0', ATENEA_COORD_ANCHORS=json.dumps([root]),
          ATENEA_COORD_RELAYS=json.dumps([{'relay_id': relay_id, 'name':'lab', 'udp':'198.51.100.10:443',
                                         'tls':'198.51.100.10:443', 'tls_name':'relay.atenea.one'}]))
    spawn('inet', 'atenea-relay', 'relay.log', ATENEA_RELAY_ID=relay_id, ATENEA_RELAY_UDP_LISTEN='0.0.0.0:443',
          ATENEA_RELAY_TCP_LISTEN='0.0.0.0:443', ATENEA_RELAY_COORDINATOR_KEYS=json.dumps([pub]),
          ATENEA_RELAY_TLS_CERT_FILE='/tmp/lab/certs/relay.pem',
          ATENEA_RELAY_TLS_KEY_FILE='/tmp/lab/certs/relay.key',
          ATENEA_RELAY_STATUS_SOCKET='/tmp/lab/relay.sock')
    for i, name in enumerate(names, 1):
        spec = f'network_id = "{net}"\ndevice_bundle = "/tmp/lab/{name}/bundle.bin"\nvalidity_secs = 900\ntunnel_addrs = ["10.77.0.{i}"]\nnetwork_prefixes = ["10.77.0.0/24"]\n'
        for j, other in enumerate(names, 1):
            if other != name:
                spec += f'[[peer]]\nbundle = "/tmp/lab/{other}/bundle.bin"\naddrs = ["10.77.0.{j}"]\n'
        spec += '[[rule]]\nsrc = "any"\ndst = "any"\n'
        (LAB / (name + '.spec')).write_text(spec)
        run('atenea', 'issue', '--issuer-key', '/tmp/lab/issuer/issuer.key', '--issuer-cert',
            '/tmp/lab/issuer/issuer.cert', '--spec', '/tmp/lab/' + name + '.spec', '--out', '/tmp/lab/' + name + '.cfg')
        (LAB / (name + '.toml')).write_text(f'''interface = "atenea0"
listen = "0.0.0.0:51820"
ipc_socket = "/tmp/lab/{name}.sock"
noise_key_file = "/tmp/lab/{name}/noise.key"
identity_key_file = "/tmp/lab/{name}/identity.key"
config_file = "/tmp/lab/{name}.cfg"
state_dir = "/tmp/lab/{name}.state"
network_id = "{net}"
device_id = "{ids[name]}"
trust_anchor = "{root}"
[coordinator]
url = "http://198.51.100.10:8081"
insecure_http = true
stun = ["198.51.100.10:3478", "198.51.100.10:3479"]
relay_ca_file = "/tmp/lab/certs/ca.pem"
''')
        run('ip', 'netns', 'exec', name, 'atenea', 'up', '--config', '/tmp/lab/' + name + '.toml', '--log', '/tmp/lab/' + name + '.log')
    began = time.monotonic()
    pings = []
    for i, name in enumerate(names, 1):
        for j in range(i + 1, N + 1):
            pings.append(subprocess.Popen(['ip', 'netns', 'exec', name, 'ping', '-n', '-i', '.5', f'10.77.0.{j}'],
                                          stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL))
    processes.extend(pings)
    seen = set()
    states = {}
    deadline = began + 155
    while time.monotonic() < deadline:
        states = {name: status(name) for name in names}
        for a, b in itertools.combinations(names, 2):
            pair = (a, b)
            peers = [next(p for p in states[x]['peers'] if bytes(p['device_id']).hex() == ids[y].replace('-', '')) for x, y in ((a,b),(b,a))]
            paths = [p['path'] for p in peers]
            if (pair not in seen and all(p in ('direct', 'relay-udp', 'relay-tls') for p in paths)
                    and all(p['rx_bytes'] > 0 and p['tx_bytes'] > 0 for p in peers)):
                seen.add(pair)
                emit('PAIR', n=N, blocked=BLOCK, pair=pair, elapsed=round(time.monotonic()-began,3), paths=paths)
        if len(seen) == N * (N - 1) // 2:
            break
        time.sleep(1)
    for name, state in states.items():
        emit('STATUS', n=N, blocked=BLOCK, agent=name, state=state)
    emit('RELAY', n=N, blocked=BLOCK, state=relay_status())
    metrics = run('ip', 'netns', 'exec', 'inet', 'python3', '-c',
                  'import urllib.request; print(urllib.request.urlopen("http://127.0.0.1:9102/metrics").read().decode())')
    emit('COORDINATOR', n=N, blocked=BLOCK, metrics=metrics)
    for name in ['coord', 'relay'] + names:
        lines = (LAB / (name + '.log')).read_text().splitlines()
        emit('LOG', n=N, blocked=BLOCK, agent=name, lines=lines[-12:])
    pending = [(name, p['path']) for name, state in states.items() for p in state['peers']
               if p['path'] not in ('direct', 'relay-udp', 'relay-tls')]
    complete = len(seen) == N*(N-1)//2 and not pending
    emit('RESULT', n=N, blocked=BLOCK, established=len(seen), pairs=N*(N-1)//2,
         elapsed=round(time.monotonic()-began,3), complete=complete, pending=pending,
         max_per_ip=int(os.environ.get('ATENEA_RELAY_MAX_PER_IP', '32')))
    if not complete:
        sys.exit(1)
finally:
    for p in processes:
        p.terminate()
