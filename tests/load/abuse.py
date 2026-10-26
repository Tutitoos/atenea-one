#!/usr/bin/env python3
"""Sondas de abuso reales: no cuentan MAC1/cookie como autorización Noise."""
import hashlib
import json
import os
from pathlib import Path
import socket
import struct
import subprocess
import sys
import threading
import time
sys.path.insert(0, '/src/tests/load')
from traffic import Measure, command, emit, ns, state, path, transfer

ROOT=Path('/tmp/lab')

def mac(key, data):
    return hashlib.blake2s(data,key=key,digest_size=16).digest()

def stop(name):
    config=str(ROOT/(name+'.toml')).encode()
    pids=[]
    for path in Path('/proc').glob('[0-9]*/cmdline'):
        try:
            args=path.read_bytes().split(b'\0')
            if config in args and b'run' in args:pids.append(path.parent)
        except FileNotFoundError:pass
    began=time.monotonic()
    command('atenea','down','--socket',str(ROOT/(name+'.sock')))
    deadline=time.monotonic()+10
    def alive():
        for pid in pids:
            try:
                if (pid/'cmdline').read_bytes():return True
            except FileNotFoundError:pass
        return False
    while (ROOT/(name+'.sock')).exists() or alive():
        assert time.monotonic()<deadline, 'Parada no completada: '+name
        time.sleep(.05)
    return time.monotonic()-began

def start(name):
    try:
        command('ip','netns','exec',name,'atenea','up','--config',str(ROOT/(name+'.toml')),'--log',str(ROOT/(name+'.log')))
    except subprocess.CalledProcessError:
        print((ROOT/(name+'.log')).read_text()[-4000:],file=sys.stderr)
        raise
    deadline=time.monotonic()+10
    while not (ROOT/(name+'.sock')).exists():
        assert time.monotonic()<deadline, 'Arranque no completado: '+name+' '+(ROOT/(name+'.log')).read_text()[-1000:]
        time.sleep(.05)

def worker(mode):
    public=(ROOT/'b/bundle.bin').read_bytes()[18:50]
    key=hashlib.blake2s(b'label-mac1'+public).digest()
    prefix=b'\x02\x01'+struct.pack('!I',12345)+bytes(108)
    tag=mac(key,prefix)
    packet=prefix+tag+bytes(16)
    sock=socket.socket(socket.AF_INET,socket.SOCK_DGRAM)
    sock.bind(('127.0.0.2',0));sock.setblocking(False)
    replies=0;lengths=set();sent=0
    if mode=='invalid-mac1': packet=prefix+bytes(32)
    raw = socket.socket(socket.AF_INET,socket.SOCK_RAW,socket.IPPROTO_RAW) if mode=='invalid-mac1' else None
    if mode=='valid-cookie':
        deadline=time.monotonic()+3
        while time.monotonic()<deadline:
            for _ in range(200):sock.sendto(packet,('127.0.0.1',51820))
            try:
                reply,_=sock.recvfrom(2048)
                if reply[:2]==b'\x02\x04':break
            except BlockingIOError:time.sleep(.005)
        else:raise RuntimeError('No COOKIE_REPLY')
        cookie=bytes.fromhex(command('atenea-load-sign','wire-cookie',public.hex(),packet.hex(),reply.hex()).strip())
        packet=packet[:130]+mac(cookie,packet[:130])
    sockets=[sock]
    if mode=='valid-mac1-no-cookie':
        for index in range(3,11):
            extra=socket.socket(socket.AF_INET,socket.SOCK_DGRAM);extra.bind(('127.0.0.'+str(index),0));extra.setblocking(False);sockets.append(extra)
    rate=int(sys.argv[3])
    start=time.monotonic();end=start+20
    while time.monotonic()<end:
        # Tasas ofrecidas: CPU del generador separada de la del respondedor.
        target=min(rate*20,int((time.monotonic()-start)*rate)+max(1,rate//1000)) if rate else 0
        while sent<target:
            if raw:
                source=socket.inet_aton('198.18.'+str((sent//256)%256)+'.'+str(sent%256))
                body=struct.pack('!HHHH',40000,51820,8+len(packet),0)+packet
                ip=struct.pack('!BBHHHBBH4s4s',0x45,0,20+len(body),1,0,64,17,0,source,socket.inet_aton('127.0.0.1'))
                raw.sendto(ip+body,('127.0.0.1',0))
            else:
                sockets[sent%len(sockets)].sendto(packet,('127.0.0.1',51820))
            sent+=1
        for receiver in sockets:
            while True:
                try:
                    reply,_=receiver.recvfrom(2048)
                    if reply[:2]==b'\x02\x04':replies+=1;lengths.add(len(reply))
                except BlockingIOError:break
        time.sleep(.001)
    print(json.dumps(dict(sent=sent,elapsed_s=time.monotonic()-start,cookie_replies=replies,reply_lengths=sorted(lengths),returnable_sources=len(sockets))))

def handshake():
    command('bash','-ec','source tests/network-lab/nat/lib.sh; ip netns add c; ip -n c link set lo up; _topo_host c 1 22')
    command('atenea','device-init','--dir',str(ROOT/'c'))
    spec=(ROOT/'a.spec.toml').read_text().replace('/a/bundle.bin','/c/bundle.bin').replace('10.77.0.1','10.77.0.3').replace('fd77::1','fd77::3')
    (ROOT/'c.spec.toml').write_text(spec+'\n[[peer]]\nbundle = "/tmp/lab/a/bundle.bin"\naddrs = ["10.77.0.1", "fd77::1"]\n')
    for name in ('a','b'):
        with (ROOT/(name+'.spec.toml')).open('a') as file:
            file.write('\n[[peer]]\nbundle = "/tmp/lab/c/bundle.bin"\naddrs = ["10.77.0.3", "fd77::3"]\n')
    for name in ('a','b','c'):
        command('atenea','issue','--issuer-key',str(ROOT/'issuer/issuer.key'),'--issuer-cert',str(ROOT/'issuer/issuer.cert'),'--spec',str(ROOT/(name+'.spec.toml')),'--out',str(ROOT/(name+'.cfg')))
    conf=(ROOT/'a.toml').read_text().replace('/a','/c').replace((ROOT/'a/bundle.bin').read_bytes()[2:18].hex(),(ROOT/'c/bundle.bin').read_bytes()[2:18].hex())
    (ROOT/'c.toml').write_text(conf)
    stop('b')
    start('b')
    time.sleep(1)
    experiments=[('control',0,n) for n in (1,2,3)]
    experiments += [(mode,rate,n) for mode in ('invalid-mac1','valid-mac1-no-cookie','valid-cookie') for rate in (1000,10000,50000) for n in (1,2)]
    retry_audit=os.environ.get('ATENEA_LOAD_RETRY_AUDIT')=='1'
    if retry_audit:
        rates=[int(value) for value in os.environ.get('ATENEA_LOAD_RETRY_RATES','50000').split()]
        experiments=[('control',0,n) for n in (1,2,3)]+[('valid-mac1-no-cookie',rate,n) for rate in rates for n in range(6)]
    for mode,rate,repetition in experiments:
        shutdown_wait_s=stop('a')
        emit('agent-shutdown',mode=mode,offered_pps=rate,repetition=repetition,elapsed_s=shutdown_wait_s,ok=shutdown_wait_s<3)
        before=state('b')['counters']
        timings={}
        timeline=[]
        def first_ping(label,namespace,target):
            began=time.monotonic()
            while time.monotonic()-began<60:
                result=subprocess.run(['ip','netns','exec',namespace,'ping','-c','1','-W','1',target],stdout=subprocess.DEVNULL,stderr=subprocess.DEVNULL)
                if result.returncode==0:
                    timings[label]=dict(ok=True,first_ping_s=time.monotonic()-began)
                    return
                time.sleep(.1)
            timings[label]=dict(ok=False,first_ping_s=time.monotonic()-began)
        with Measure() as measure:
            capture_path=ROOT/('handshake-'+mode+'-'+str(rate)+'-'+str(repetition)+'.pcap')
            capture_unix=time.time()
            capture_a_path=ROOT/('a-'+capture_path.name)
            capture_a=None
            if retry_audit:
                capture_a=subprocess.Popen(['ip','netns','exec','a','tcpdump','-i','eth0','-n','-U','-w',str(capture_a_path),'udp'],stdout=subprocess.DEVNULL,stderr=subprocess.DEVNULL)
            capture=subprocess.Popen(['ip','netns','exec','b','tcpdump','-i','eth0','-n','-U','-w',str(capture_path),'udp'],stdout=subprocess.DEVNULL,stderr=subprocess.DEVNULL)
            deadline=time.monotonic()+3
            while not capture_path.exists():
                assert capture.poll() is None and time.monotonic()<deadline, 'Captura LAN no preparada'
                time.sleep(.02)
            attack=subprocess.Popen(['ip','netns','exec','b','python3',__file__,'worker',mode,str(rate)],stdout=subprocess.PIPE,text=True)
            time.sleep(1)
            start('a')
            start('c')
            probes=[threading.Thread(target=first_ping,args=args) for args in (('a-b','a','10.77.0.2'),('c-b','c','10.77.0.2'),('a-c','a','10.77.0.3'))]
            for thread in probes:thread.start()
            began=time.monotonic();previous=began
            while any(thread.is_alive() for thread in probes) or attack.poll() is None:
                current=time.monotonic()
                status=state('b')
                timeline.append(dict(elapsed_s=current-began,observer_gap_s=current-previous,counters=status['counters'],
                                     peers=[dict(path=p['path'],last_rx_secs=p['last_rx_secs']) for p in status['peers']]))
                if retry_audit:
                    timeline[-1]['a_counters']=state('a')['counters']
                previous=current
                time.sleep(.5)
            for thread in probes:thread.join()
            result=json.loads(attack.communicate(timeout=5)[0]);assert attack.returncode==0
            capture.terminate();capture.wait(timeout=5)
            if capture_a:
                capture_a.terminate();capture_a.wait(timeout=5)
        after=state('b')['counters']
        emit('handshake-flood',mode=mode,offered_pps=rate,repetition=repetition,shutdown_wait_s=shutdown_wait_s,timings=timings,timeline=timeline,
             legitimate_ok=timings['a-b']['ok'],legitimate_handshake_s=timings['a-b']['first_ping_s'],
             counter_delta={k:after[k]-before.get(k,0) for k in after if isinstance(after[k],int)},resources=measure.resources,**result)
        emit('handshake-keepalives',mode=mode,offered_pps=rate,repetition=repetition,**keepalives(capture_path))
        if retry_audit:
            from handshake_trace import analyze
            import shutil
            traces={}
            for name,pcap in (('a',capture_a_path),('b',capture_path)):
                interfaces=json.loads(ns(name,'ip','-j','-4','addr','show','eth0'))
                own_ip=next(info['local'] for interface in interfaces for info in interface['addr_info'] if info['family']=='inet')
                traces[name]=analyze(pcap,own_ip,capture_unix)
                shutil.copyfile(pcap,Path('/results')/('retry-'+name+'-'+mode+'-'+str(repetition)+'.pcap'))
            counters={name:state(name)['counters'] for name in ('a','b')}
            missing={name:[key for key in ('rx_cookie_reply','tx_cookie_reply') if key not in values] for name,values in counters.items()}
            emit('handshake-retry-audit',mode=mode,offered_pps=rate,repetition=repetition,warmup=repetition==0,
                 traces=traces,final_counters=counters,missing_status_counters=missing,timings=timings)
        stop('c')
        assert all(n==62 for n in result['reply_lengths'])
        assert result['cookie_replies']<=100+100*result['elapsed_s']
        time.sleep(2)

def keepalives(path):
    groups={}
    with path.open('rb') as stream:
        magic=stream.read(24)[:4]
        endian='<' if magic==b'\xd4\xc3\xb2\xa1' else '>'
        while header:=stream.read(16):
            sec,usec,size,_=struct.unpack(endian+'IIII',header)
            packet=stream.read(size)
            if len(packet)<42 or packet[12:14]!=b'\x08\x00' or packet[23]!=17:continue
            offset=14+(packet[14]&15)*4
            data=packet[offset+8:]
            if len(data)==30 and data[:2]==b'\x02\x03':
                key=packet[26:34].hex()+packet[offset:offset+4].hex()
                groups.setdefault(key,[]).append(sec+usec/1e6)
    gaps=[b-a for times in groups.values() for a,b in zip(times,times[1:])]
    return dict(empty_data_packets=sum(map(len,groups.values())),observed_gaps_s=gaps,max_gap_s=max(gaps,default=0))

def relay_worker():
    relay=bytes.fromhex((ROOT/'relay-id').read_text().strip())
    packet=b'\xa1\x02\x01'+relay+bytes(45)
    sockets=[];received=0
    start=time.monotonic()
    for n in range(5000):
        s=socket.socket(socket.AF_INET,socket.SOCK_DGRAM)
        s.bind(('127.'+str(1+n//65536)+'.'+str((n//256)%256)+'.'+str(n%256),0))
        s.settimeout(.02)
        s.sendto(packet,('127.0.0.1',443))
        try:
            reply,_=s.recvfrom(2048)
            assert len(reply)==35 and reply[:3]==b'\xa1\x02\x02'
            received+=1
        except socket.timeout:pass
        s.close()
    # La tabla llena debe recuperar cubos repuestos y permitir un origen nuevo.
    time.sleep(1.1)
    s=socket.socket(socket.AF_INET,socket.SOCK_DGRAM);s.bind(('127.250.250.250',0));s.settimeout(1)
    s.sendto(packet,('127.0.0.1',443))
    try:reply,_=s.recvfrom(2048);fresh=len(reply)==35
    except socket.timeout:fresh=False
    print(json.dumps(dict(sources=5000,responses=received,new_source_served=fresh,elapsed_s=time.monotonic()-start)))

def relay():
    for repetition in (1,2):
        with Measure() as measure:
            value=json.loads(ns('inet','python3',__file__,'relay-worker'))
        emit('relay-cookie-flood',repetition=repetition,resources=measure.resources,**value)
        assert value['new_source_served'],'Regresión F1: IP nueva sin servicio'
    path('relay-udp')
    for repetition in (1,2):
        s=socket.socket(socket.AF_UNIX);s.connect('/tmp/lab/relay.sock');snapshot=json.loads(s.makefile().readline());s.close()
        emit('relay-pair-bandwidth',repetition=repetition,pairs=1,relay_status=snapshot,**transfer())

if __name__=='__main__':
    if sys.argv[1]=='worker':worker(sys.argv[2])
    elif sys.argv[1]=='relay-worker':relay_worker()
    elif sys.argv[1]=='handshake':handshake()
    elif sys.argv[1]=='relay':relay()
