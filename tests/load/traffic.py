#!/usr/bin/env python3
# Copyright 2026 Tutitoos
# SPDX-License-Identifier: Apache-2.0
"""Carga acotada en memoria, reloj monotónico y recursos Linux por proceso."""
import hashlib
import json
import math
import os
from pathlib import Path
import re
import socket
import struct
import signal
import subprocess
import sys
import threading
import time

HERE = str(Path(__file__).resolve())
RESULTS = Path('/results')
PEER_IP = os.environ.get('ATENEA_LOAD_PEER_IP', '10.77.0.2')

def emit(kind, **values):
    row = dict(kind=kind, wall_unix=time.time(), **values)
    print(json.dumps(row), flush=True)
    with (RESULTS / (sys.argv[1] + '.jsonl')).open('a') as file:
        file.write(json.dumps(row) + '\n')

def command(*args, **kwargs):
    return subprocess.check_output(list(args), text=True, **kwargs)

def ns(name, *args):
    return command('ip', 'netns', 'exec', name, *args)

def state(name):
    return json.loads(command('atenea', 'status', '--socket', '/tmp/lab/'+name+'.sock'))

def processes():
    found = {}
    for directory in Path('/proc').glob('[0-9]*'):
        try:
            comm = (directory / 'comm').read_text().strip()
            if not comm.startswith(('atenea-agent', 'atenea-relay', 'atenea-coord')):
                continue
            stat = (directory / 'stat').read_text().rsplit(')', 1)[1].split()
            status = (directory / 'status').read_text()
            rss = int(re.search(r'VmRSS:\s+(\d+)', status)[1])
            found[str(directory.name)] = dict(name=comm, cpu_s=(int(stat[11])+int(stat[12]))/os.sysconf('SC_CLK_TCK'), rss_kib=rss, fds=len(list((directory/'fd').iterdir())))
        except (OSError, TypeError):
            continue
    return found

class Measure:
    def __enter__(self):
        self.begin = time.monotonic()
        self.first = processes()
        self.samples = [self.first]
        self.stop = threading.Event()
        def sample():
            while not self.stop.wait(.25):
                self.samples.append(processes())
        self.thread = threading.Thread(target=sample)
        self.thread.start()
        return self
    def __exit__(self, *unused):
        self.stop.set()
        self.thread.join()
        self.elapsed = time.monotonic()-self.begin
        last = processes()
        self.resources = {}
        for pid, first in self.first.items():
            if pid not in last:
                self.resources[pid] = dict(name=first['name'], exited_during_measurement=True)
                continue
            cpu = last[pid]['cpu_s']-first['cpu_s']
            self.resources[pid] = dict(name=first['name'], cpu_s=cpu, cpu_percent=100*cpu/self.elapsed,
                rss_peak_kib=max(s.get(pid, first)['rss_kib'] for s in self.samples+[last]), fds=last[pid]['fds'])

def transfer(mib=512):
    server = subprocess.Popen(['ip','netns','exec','b','python3',HERE,'tcp-server'], stdout=subprocess.PIPE, text=True)
    try:
        assert server.stdout.readline().strip() == 'ready'
        with Measure() as measure:
            client = json.loads(ns('a','python3',HERE,'tcp-client',str(mib)))
            received = json.loads(server.communicate(timeout=180)[0])
        assert received == client, (received,client)
        rate = client['bytes']*8/measure.elapsed/1e6
        for value in measure.resources.values():
            value['cpu_percent_per_mbit_s'] = value['cpu_percent']/rate
            value['rss_kib_per_mbit_s'] = value['rss_peak_kib']/rate
        return dict(**client, elapsed_s=measure.elapsed, mbit_s=rate, resources=measure.resources)
    finally:
        if server.poll() is None:
            server.kill()
            server.wait()

def pings(target):
    with Measure() as measure:
        text = ns('a','ping','-n','-c','2000','-i','0.01','-W','1',target)
    values = sorted(float(v) for v in re.findall(r'time[=<]([\d.]+)',text))
    assert len(values) > 0
    return dict(sent=2000,received=len(values),loss_percent=100*(2000-len(values))/2000,
        p50_ms=values[math.ceil(.50*len(values))-1],p95_ms=values[math.ceil(.95*len(values))-1],
        p99_ms=values[math.ceil(.99*len(values))-1],elapsed_s=measure.elapsed,resources=measure.resources)

def path(mode):
    function = {'direct':'net_unblock_all','relay-udp':'net_block_p2p','relay-tls':'net_block_udp'}[mode]
    command('bash','-c','source tests/network-lab/nat/lib.sh; '+function)
    if mode != 'direct':
        # El bloqueador compartido corta UDP exterior; no debe cortar UDP dentro de TUN.
        for namespace in ('a','b'):
            ns(namespace,'nft','insert','rule','inet','relay_lab','early_output','oifname','atenea0','accept')
            ns(namespace,'nft','insert','rule','inet','relay_lab','early_input','iifname','atenea0','accept')
    deadline = time.monotonic()+100
    while time.monotonic() < deadline:
        subprocess.run(['ip','netns','exec','a','ping','-c','1','-W','1',PEER_IP],stdout=subprocess.DEVNULL,stderr=subprocess.DEVNULL)
        peers = [state(n)['peers'] for n in ('a','b')]
        if all(p and p[0]['path'] == mode for p in peers):
            return
        time.sleep(.5)
    raise RuntimeError('Sin camino '+mode+': '+json.dumps({n:dict(status=state(n),log=Path('/tmp/lab/'+n+'.log').read_text(errors='replace')[-6000:]) for n in ('a','b')}))

def udp():
    server = subprocess.Popen(['ip','netns','exec','b','python3',HERE,'udp-server'],stdout=subprocess.PIPE,text=True)
    try:
        assert server.stdout.readline().strip() == 'ready'
        with Measure() as measure:
            ns('a','python3',HERE,'udp-client')
            received = json.loads(server.communicate(timeout=15)[0])
        return dict(sent=10000,received=received,loss_percent=(10000-received)/100,elapsed_s=measure.elapsed,
                    offered_mbit_s=10000*1200*8/5/1e6,received_mbit_s=received*1200*8/5/1e6,resources=measure.resources)
    finally:
        if server.poll() is None:
            server.kill()
            server.wait()

def tunnel():
    # Comparador subyacente: ida/vuelta a inet, no un trayecto idéntico al P2P.
    if os.environ.get('ATENEA_LOAD_BASELINE','1') == '1':
        for repetition in (1,2):
            emit('baseline',repetition=repetition,target='198.51.100.10',**pings('198.51.100.10'))
    for mode in os.environ.get('ATENEA_LOAD_PATHS','direct relay-udp relay-tls').split():
        path(mode)
        for repetition in (1,2):
            emit('tcp',path=mode,repetition=repetition,**transfer())
            emit('rtt',path=mode,repetition=repetition,**pings(PEER_IP))
            emit('udp',path=mode,repetition=repetition,**udp())

def soak_loop(seconds, iteration, *, clock=time.monotonic, sleep=time.sleep, report=emit):
    """Cadencia absoluta: una avería no desplaza ni termina las siguientes muestras."""
    started = clock()
    tick = 0
    failures = []
    while clock() < started + seconds:
        try:
            iteration(tick % 6 == 0, started + seconds - clock())
        except Exception as error:
            failures.append(str(error))
            report('soak-error', elapsed_s=clock()-started, error=repr(error))
        tick += 1
        sleep(max(0, min(started + tick*10, started + seconds)-clock()))
    return failures

def soak(hours):
    warmup = {n: state(n) for n in ('a', 'b')}
    emit('soak-warmup', phase='before-direct', agent_states=warmup)
    path('direct')
    emit('soak-warmup', phase='direct', agent_states={n:state(n) for n in ('a','b')})
    path('relay-udp')
    steady_since = time.time()
    baseline = {n: state(n) for n in ('a', 'b')}
    emit('soak-steady', steady_since=steady_since, expected_blocked_path='direct',
        agent_states=baseline, warmup_counters={n:s['counters'] for n,s in warmup.items()})
    started = time.monotonic()
    initial = processes()
    samples = []
    metric_baseline = None
    def expired(signum, frame):
        raise TimeoutError('Iteración soak excedió su plazo')
    previous_handler = signal.signal(signal.SIGALRM, expired)
    ping_log = (RESULTS/'soak-ping.log').open('w')
    continuous = subprocess.Popen(['ip','netns','exec','a','ping','-D','-n','-i','.2',PEER_IP],stdout=ping_log,stderr=subprocess.STDOUT)
    def collect(do_transfer, remaining):
        nonlocal metric_baseline
        if do_transfer:
            emit('soak-transfer', **transfer(16))
        current = processes()
        samples.append(current)
        errors = {}
        for log in Path('/tmp/lab').glob('*.log'):
            text = log.read_text(errors='replace')
            errors[log.name] = len(re.findall(r'(?i)\b(error|panic|fatal)\b',text))
        assert continuous.poll() is None, 'Tráfico continuo detenido'
        assert set(initial).issubset(current), 'Proceso reiniciado/desaparecido durante soak'
        assert not any(errors.values()), 'Error/panic/fatal en logs del soak'
        metrics = {}
        if do_transfer:
            import urllib.request
            for service, variable, port in (('control-plane','CP_IP',9101),('coordinator','COORD_IP',9102),('relay','EDGE_IP',9103)):
                host = os.environ.get(variable)
                if host:
                    with urllib.request.build_opener(urllib.request.ProxyHandler({})).open('http://'+host+':'+str(port)+'/metrics',timeout=5) as response:
                        metrics[service] = response.read().decode()
            error_metrics={}
            for service,text in metrics.items():
                for line in text.splitlines():
                    if line.startswith('atenea_database_errors_total ') or (line.startswith('atenea_http_requests_total{') and re.search(r'code="5[0-9][0-9]"',line)):
                        key,value=line.rsplit(' ',1)
                        error_metrics[service+'/'+key]=float(value)
            if metric_baseline is not None:
                assert all(math.isfinite(value) and value<=metric_baseline.get(key,0) for key,value in error_metrics.items()), 'Aumento de errores DB/HTTP5xx en Prometheus'
            else:
                metric_baseline=error_metrics
            emit('soak-prometheus',elapsed_s=time.monotonic()-started,metrics=metrics)
        agent_states = {n:state(n) for n in ('a','b')}
        emit('soak-sample',elapsed_s=time.monotonic()-started,processes=current,log_errors=errors,
            agent_counters={n:s['counters'] for n,s in agent_states.items()}, agent_states=agent_states)
    def iteration(do_transfer, remaining):
        signal.setitimer(signal.ITIMER_REAL, min(55, remaining))
        try:
            collect(do_transfer, remaining)
        finally:
            signal.setitimer(signal.ITIMER_REAL, 0)
    try:
        failures = soak_loop(hours*3600, iteration)
    finally:
        signal.setitimer(signal.ITIMER_REAL, 0)
        signal.signal(signal.SIGALRM, previous_handler)
        continuous.send_signal(signal.SIGINT)
        continuous.wait(timeout=5)
        ping_log.close()
    final = processes()
    growth = {pid:dict(rss_growth_kib=final[pid]['rss_kib']-first['rss_kib'],fds_growth=final[pid]['fds']-first['fds']) for pid,first in initial.items() if pid in final}
    emit('soak-summary',elapsed_s=time.monotonic()-started,requested_s=hours*3600,samples=len(samples),growth=growth,
        steady_since=steady_since, missing_processes=sorted(set(initial)-set(final)), failures=failures)
    assert not failures, f'{len(failures)} iteraciones fallidas durante soak'
    assert not set(initial)-set(final), 'Proceso perdido'
    assert all(g['rss_growth_kib'] <= 16384 and g['fds_growth'] <= 8 for g in growth.values()), 'Crecimiento superior al margen local: 16MiB RSS/8 fds'

if __name__ == '__main__':
    mode = sys.argv[1]
    if mode == 'tcp-server':
        s=socket.socket();s.setsockopt(socket.SOL_SOCKET,socket.SO_REUSEADDR,1);s.bind((PEER_IP,9000));s.listen();s.settimeout(180)
        print('ready',flush=True)
        c,_=s.accept();c.settimeout(180);h=hashlib.sha256();count=0
        while True:
            data=c.recv(65536)
            if not data: break
            h.update(data);count+=len(data)
        c.close();print(json.dumps(dict(bytes=count,sha256=h.hexdigest())))
    elif mode == 'tcp-client':
        s=socket.create_connection((PEER_IP,9000),timeout=180)
        block=bytes(range(256))*256;h=hashlib.sha256();count=0
        for _ in range(int(sys.argv[2])*16):
            s.sendall(block);h.update(block);count+=len(block)
        s.shutdown(socket.SHUT_WR);s.recv(1);print(json.dumps(dict(bytes=count,sha256=h.hexdigest())))
    elif mode == 'udp-server':
        s=socket.socket(socket.AF_INET,socket.SOCK_DGRAM);s.setsockopt(socket.SOL_SOCKET,socket.SO_RCVBUF,4194304);s.bind((PEER_IP,9001));s.settimeout(3)
        print('ready',flush=True);got=set()
        try:
            while True: got.add(struct.unpack('!I',s.recv(2048)[:4])[0])
        except socket.timeout: pass
        print(json.dumps(len(got)))
    elif mode == 'udp-client':
        s=socket.socket(socket.AF_INET,socket.SOCK_DGRAM);start=time.monotonic()
        for n in range(10000):
            s.sendto(struct.pack('!I',n)+bytes(1196),(PEER_IP,9001))
            delay=start+(n+1)*.0005-time.monotonic()
            if delay>0:time.sleep(delay)
    elif mode == 'tunnel': tunnel()
    elif mode == 'soak': soak(float(sys.argv[2]))
    else:
        import runpy
        runpy.run_path('/src/tests/load/abuse.py',run_name='__main__')
