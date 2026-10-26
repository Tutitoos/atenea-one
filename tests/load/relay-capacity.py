#!/usr/bin/env python3
# Copyright 2026 Tutitoos
# SPDX-License-Identifier: Apache-2.0
"""Capacidad real con tokens v2, identidades efímeras y carga DATA opaca por N pares."""
import json
import os
from pathlib import Path
import socket
import struct
import subprocess
import sys
import threading
import time
sys.path.insert(0,'/src/tests/load')
from traffic import Measure,command,emit,processes

LAB=Path('/tmp/lab')

def run():
    # Proceso propio: cuotas por defecto, tabla reducida SOLO para regresión de recuperación F1.
    relay_id=(LAB/'relay-id').read_text().strip()
    seed=(LAB/'relay-signing.hex').read_text().strip()
    public=command('atenea-coordinator','--print-relay-key').strip()
    env=os.environ|dict(ATENEA_RELAY_ID=relay_id,ATENEA_RELAY_UDP_LISTEN='[::]:444',ATENEA_RELAY_TCP_LISTEN='127.0.0.1:8444',
        ATENEA_RELAY_COORDINATOR_KEYS=json.dumps([public]),ATENEA_RELAY_TLS_CERT_FILE=str(LAB/'certs/relay.pem'),
        ATENEA_RELAY_TLS_KEY_FILE=str(LAB/'certs/relay.key'),ATENEA_RELAY_STATUS_SOCKET=str(LAB/'capacity.sock'),ATENEA_RELAY_METRICS_LISTEN='127.0.0.1:9104',ATENEA_RELAY_MAX_RATE_ENTRIES='65536')
    log=(LAB/'capacity.log').open('w')
    relay=subprocess.Popen(['atenea-relay'],env=env,stdout=log,stderr=subprocess.STDOUT)
    signer=subprocess.Popen(['atenea-load-sign','raw-sign'],stdin=subprocess.PIPE,stdout=subprocess.PIPE,text=True)
    def sign(key,message):
        signer.stdin.write(key+'\t'+message.hex()+'\n');signer.stdin.flush()
        return bytes.fromhex(signer.stdout.readline().strip())
    def status():
        s=socket.socket(socket.AF_UNIX);s.connect(str(LAB/'capacity.sock'))
        result=json.loads(s.makefile().readline());s.close();return result
    rid=bytes.fromhex(relay_id)
    device_seed=(LAB/'a/identity.key').read_text().strip()
    identity=(LAB/'a/bundle.bin').read_bytes()[50:82]
    sockets=[]
    cookie_timeouts=0
    def allocation(number):
        nonlocal cookie_timeouts
        s=socket.socket(socket.AF_INET6,socket.SOCK_DGRAM)
        # Cada /64 es un origen; no se oculta un límite por IP bajo un total artificial.
        address='fd88:'+format(number+1,'x')+'::1'
        command('ip','-6','addr','replace',address+'/128','dev','lo','nodad')
        s.bind((address,0));s.settimeout(1)
        for attempt in range(3):
            s.sendto(b'\xa1\x02\x01'+rid+bytes(45),('::1',444))
            try:
                response=s.recv(2048)
                assert len(response)==35 and response[:19]==b'\xa1\x02\x02'+rid
                cookie=response[19:35]
                break
            except socket.timeout:
                cookie_timeouts+=1
        else:
            raise RuntimeError(f'COOKIE agotado: asignación {number}, relay_exit={relay.poll()}, estado={status()}')
        src=(number+1).to_bytes(16,'big');dst=(number+2 if number%2==0 else number).to_bytes(16,'big')
        network=(1+number//512).to_bytes(16,'big')
        now=int(time.time())
        token=b'AORT\x02'+rid+network+src+dst+identity+struct.pack('!QQQ',now,now+600,number+1)
        token+=sign(seed,b'atenea-one/relay-token/v2\0'+token)
        proof=sign(device_seed,b'atenea-one/relay-alloc/v2\0'+rid+cookie+struct.pack('!Q',now)+token)
        packet=b'\xa1\x02\x03'+rid+cookie+struct.pack('!QH',now,189)+token+proof
        s.sendto(packet,('::1',444))
        while True:
            reply=s.recv(2048)
            if reply[:3] in (b'\xa1\x02\x04',b'\xa1\x02\x05'):break
        if reply[:3]==b'\xa1\x02\x04':
            allocation_id=reply[3:11];sockets.append((s,allocation_id))
            return True
        s.close()
        assert reply==b'\xa1\x02\x05\x04',reply.hex()
        return False
    try:
        deadline=time.monotonic()+10
        while not (LAB/'capacity.sock').exists():
            assert relay.poll() is None,(LAB/'capacity.log').read_text()
            assert time.monotonic()<deadline
            time.sleep(.05)
        baseline=status()
        empty_rss=processes()[str(relay.pid)]['rss_kib']
        began=time.monotonic();group_responses=0
        for index in range(100):
            address='fd99:1::'+format(index+1,'x')
            command('ip','-6','addr','replace',address+'/128','dev','lo','nodad')
            probe=socket.socket(socket.AF_INET6,socket.SOCK_DGRAM);probe.bind((address,0));probe.settimeout(.002)
            probe.sendto(b'\xa1\x02\x01'+rid+bytes(45),('::1',444))
            try:
                assert len(probe.recv(2048))==35
                group_responses+=1
            except socket.timeout:pass
            probe.close()
        emit('relay-ipv6-group',repetition=int(sys.argv[1]),addresses=100,prefixes=1,responses=group_responses,elapsed_s=time.monotonic()-began)
        assert group_responses <= 20+20*(time.monotonic()-began)
        with Measure() as measurement:
            successes=0
            for number in range(4097):
                if allocation(number):successes+=1
            snapshot=status()
        resident_rss=processes()[str(relay.pid)]['rss_kib']
        emit('relay-capacity',repetition=int(sys.argv[1]),accepted=successes,rejected=4097-successes,elapsed_s=measurement.elapsed,
             relay_pid=relay.pid,cookie_timeouts=cookie_timeouts,empty_rss_kib=empty_rss,resident_rss_kib=resident_rss,rss_kib_per_allocation=(resident_rss-empty_rss)/successes,
             resources=measurement.resources,status=snapshot,baseline=baseline)
        assert successes==4096,successes
        assert snapshot['allocations']==4096, 'No se alcanzaron 4096 asignaciones simultáneas'
        # N=16 pares: 32 asignaciones existentes, 1000 DATA/s/asignación durante 10 s.
        # DATA v2 válido estructuralmente, deliberadamente sin descifrado Noise en este relay.
        selected=sockets[:32];counts=[0]*len(selected);stop=threading.Event()
        def receive(index,s):
            s.settimeout(.2)
            while not stop.is_set():
                try:
                    data=s.recv(2048)
                    if data[:3]==b'\xa1\x02\x06':counts[index]+=1
                except socket.timeout:pass
        readers=[threading.Thread(target=receive,args=(n,s)) for n,(s,_) in enumerate(selected)]
        for thread in readers:thread.start()
        with Measure() as bandwidth:
            sent=0;begin=time.monotonic()
            for tick in range(10000):
                payload=b'\x02\x03'+bytes(4)+struct.pack('!Q',tick)+bytes(1186)
                for s,ident in selected:s.sendto(b'\xa1\x02\x06'+ident+payload,('::1',444));sent+=1
                delay=begin+(tick+1)*.001-time.monotonic()
                if delay>0:time.sleep(delay)
            send_elapsed=time.monotonic()-begin
            time.sleep(.3)
        stop.set()
        for thread in readers:thread.join()
        emit('relay-aggregate',repetition=int(sys.argv[1]),relay_pid=relay.pid,pairs=16,sent=sent,received=sum(counts),
            elapsed_s=bandwidth.elapsed,send_elapsed_s=send_elapsed,offered_pps=sent/send_elapsed,received_mbit_s=sum(counts)*1200*8/send_elapsed/1e6,resources=bandwidth.resources)
    finally:
        for s,_ in sockets:s.close()
        signer.terminate();signer.wait(timeout=5)
        relay.terminate();relay.wait(timeout=5);log.close()

if __name__=='__main__':run()
