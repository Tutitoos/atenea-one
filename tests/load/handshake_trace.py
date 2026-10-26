"""Cuenta retransmisiones por candidato/índice; RESP emparejada no sustituye Noise."""
import socket
import struct


def analyze(path,own_ip,began):
    attempts={};cookies=[];responses=[]
    with path.open('rb') as stream:
        magic=stream.read(24)[:4]
        assert magic in (b'\xd4\xc3\xb2\xa1',b'\xa1\xb2\xc3\xd4'), 'PCAP clásico requerido'
        endian='<' if magic==b'\xd4\xc3\xb2\xa1' else '>'
        while header:=stream.read(16):
            sec,usec,size,_=struct.unpack(endian+'IIII',header)
            packet=stream.read(size)
            if len(packet)<42 or packet[12:14]!=b'\x08\x00' or packet[23]!=17:continue
            offset=14+(packet[14]&15)*4
            sport,dport,length,_=struct.unpack('!HHHH',packet[offset:offset+8])
            data=packet[offset+8:offset+length]
            src=socket.inet_ntoa(packet[26:30]);dst=socket.inet_ntoa(packet[30:34])
            elapsed=sec+usec/1e6-began
            if data[:3]==b'\xa1\x02\x06':data=data[11:]
            if len(data)==146 and data[:2]==b'\x02\x01' and src==own_ip:
                index=int.from_bytes(data[2:6],'big')
                key=f'{dst}:{dport}/{index}'
                row=attempts.setdefault(key,dict(candidate=f'{dst}:{dport}',sender_index=index,init_sends=0,first_init_s=elapsed,first_mac2_s=None,matching_response_after_mac2_s=None))
                row['init_sends']+=1
                if any(data[130:146]) and row['first_mac2_s'] is None:row['first_mac2_s']=elapsed
            elif len(data)==62 and data[:2]==b'\x02\x04' and dst==own_ip:
                cookies.append(dict(elapsed_s=elapsed,receiver_index=int.from_bytes(data[2:6],'big'),source=f'{src}:{sport}'))
            elif len(data)==62 and data[:2]==b'\x02\x02' and dst==own_ip:
                responses.append((int.from_bytes(data[6:10],'big'),elapsed))
    for row in attempts.values():
        received=[at for index,at in responses if index==row['sender_index'] and row['first_mac2_s'] is not None and at>=row['first_mac2_s']]
        if received:row['matching_response_after_mac2_s']=min(received)
    return dict(candidates=list(attempts.values()),cookie_replies_received=cookies,
                max_init_sends_per_candidate=max((r['init_sends'] for r in attempts.values()),default=0),
                matching_response_is_inference=True,coverage='UDP IPv4 observado en eth0; TLS y otros enlaces no observables en esta captura')
