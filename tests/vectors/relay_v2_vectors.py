#!/usr/bin/env python3
"""Vectores relay-v2: Ed25519/cryptography y hashlib, independientes de Rust.

Sin red ni aleatoriedad; semillas fijas exclusivamente para reproducción.
Comparte únicamente el lector/comparador de tablas con wire_v2_vectors.py.
"""
import argparse
import hashlib
from pathlib import Path
import struct
import sys

from cryptography.hazmat.primitives.asymmetric.ed25519 import Ed25519PrivateKey
from cryptography.hazmat.primitives.serialization import Encoding, PublicFormat
from wire_v2_vectors import check_table


def generate():
    coordinator = Ed25519PrivateKey.from_private_bytes(bytes([1]) * 32)
    device = Ed25519PrivateKey.from_private_bytes(bytes([2]) * 32)
    device_public = device.public_key().public_bytes(Encoding.Raw, PublicFormat.Raw)
    relay = bytes([0x11]) * 16
    body = (b'AORT\x02' + relay + bytes([7]) * 16 + bytes([0x22]) * 16
            + bytes([0x33]) * 16 + device_public
            + struct.pack('>QQ', 1700000000, 1700000300) + bytes([0x44]) * 8)
    assert len(body) == 125
    token_domain = b'atenea-one/relay-token/v2\0'
    token_signature = coordinator.sign(token_domain + body)
    coordinator.public_key().verify(token_signature, token_domain + body)
    token = body + token_signature
    values = {'src_identity_pub': device_public, 'cuerpo125': body,
              'firma token64': token_signature, 'token189': token}
    packets = {}
    for transport, cookie in [('TCP', bytes(16)), ('UDP', bytes([0x55]) * 16)]:
        fields = relay + cookie + struct.pack('>Q', 1700000001)
        signed = b'atenea-one/relay-alloc/v2\0' + fields + token
        signature = device.sign(signed)
        device.public_key().verify(signature, signed)
        packet = b'\xa1\x02\x03' + fields + struct.pack('>H', len(token)) + token + signature
        assert len(packet) == 298
        values.update({f'entrada firmada ALLOC {transport}': signed,
                       f'firma ALLOC {transport}64': signature,
                       f'ALLOC {transport}298': packet})
        packets[transport] = packet
    # La cookie relay conserva BLAKE2s-256 concatenado y truncado, NO wire M(k,x).
    relay_cookie = hashlib.blake2s(bytes(range(32)) + relay
                                   + bytes.fromhex('c0000201ca6c')
                                   + struct.pack('>Q', 1700000000 // 60)).digest()[:16]
    values.update({'relay_cookie16': relay_cookie,
                   'COOKIE35': b'\xa1\x02\x02' + relay + relay_cookie,
                   'COOKIE_REQ64': b'\xa1\x02\x01' + relay + bytes(45),
                   'trama TCP300 (cookie cero)': struct.pack('>H', 298) + packets['TCP'],
                   'envoltura TCP300 del vector UDP (solo tramado; cookie no admitida por TCP)':
                       struct.pack('>H', 298) + packets['UDP']})
    return values


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--check', action='store_true', help='comparar con la tabla publicada')
    args = parser.parse_args()
    values = generate()
    if args.check:
        check_table(values, Path(__file__).resolve().parents[2] / 'docs/protocol/relay-v2.md')
    else:
        print('| Salida | Hex |\n| --- | --- |')
        for name, value in values.items():
            print(f'| {name} | `{value.hex()}` |')


if __name__ == '__main__':
    try:
        main()
    except ValueError as error:
        sys.exit(str(error))
