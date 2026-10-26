#!/usr/bin/env python3
"""ClientHello real y PROXY externo: ambos se comprueban contra HAProxy público."""
import os
import socket
import ssl

host = os.environ['EDGE_IP']

def connect(name, alpn):
    context = ssl.SSLContext(ssl.PROTOCOL_TLS_CLIENT)
    context.check_hostname = False
    context.verify_mode = ssl.CERT_NONE
    context.minimum_version = ssl.TLSVersion.TLSv1_3
    context.set_alpn_protocols([alpn])
    return context.wrap_socket(socket.create_connection((host, 443), timeout=8), server_hostname=name)

with connect('relay.stack.test', 'atenea-relay/2') as client:
    assert client.selected_alpn_protocol() == 'atenea-relay/2'
print('PASS SNI relay + ALPN atenea-relay/2 termina TLS en relay vía HAProxy')
try:
    with connect('unknown.stack.test', 'atenea-relay/2') as client:
        assert client.selected_alpn_protocol() != 'atenea-relay/2'
except ssl.SSLError:
    pass
print('PASS SNI desconocido no negocia relay')
try:
    with connect('relay.stack.test', 'atenea-relay/1') as client:
        assert client.selected_alpn_protocol() != 'atenea-relay/1'
except ssl.SSLError:
    pass
print('PASS ALPN relay-v1 no se negocia')
# HAProxy no lleva accept-proxy: una cabecera antepuesta nunca es ClientHello válido.
with socket.create_connection((host, 443), timeout=8) as client:
    client.sendall(b'\r\n\r\n\x00\r\nQUIT\n\x21\x11\x00\x0c' + socket.inet_aton('203.0.113.7') + socket.inet_aton('203.0.113.8') + b'\x12\x34\x01\xbb')
    try:
        assert client.recv(1) == b''
    except ConnectionResetError:
        pass
print('PASS PROXYv2 forjado externo rechazado antes de TLS')
# El listener de backend no es accesible ni a un contenedor de la red edge.
with socket.socket() as client:
    client.settimeout(2)
    assert client.connect_ex((host, 8443)) != 0
print('PASS backend PROXY relay 8443 solo loopback')
