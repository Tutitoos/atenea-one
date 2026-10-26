#!/usr/bin/env python3
"""Vectores wire-v2 independientes de Rust; solo stdlib y cryptography local.

HChaCha20 y el transcript/KDF Noise se calculan aquí; X25519 y ChaCha20Poly1305
proceden de cryptography. Nunca usar estas claves/efímeras fijas en producción.
"""
import argparse
import hashlib
import hmac
from pathlib import Path
import re
import struct
import sys

from cryptography.hazmat.primitives.asymmetric.x25519 import X25519PrivateKey
from cryptography.hazmat.primitives.ciphers.aead import ChaCha20Poly1305
from cryptography.hazmat.primitives.serialization import Encoding, PublicFormat


def digest(data):
    return hashlib.blake2s(data).digest()


def mac(key, data):
    # digest_size forma parte de los parámetros de BLAKE2s: no truncar digest32.
    return hashlib.blake2s(data, key=key, digest_size=16).digest()


def public(key):
    return key.public_key().public_bytes(Encoding.Raw, PublicFormat.Raw)


def hchacha20(key, nonce16):
    state = list(struct.unpack('<4I', b'expand 32-byte k')
                 + struct.unpack('<8I', key) + struct.unpack('<4I', nonce16))

    def rotate(value, bits):
        return ((value << bits) | (value >> (32 - bits))) & 0xffffffff

    def quarter(a, b, c, d):
        state[a] = (state[a] + state[b]) & 0xffffffff
        state[d] = rotate(state[d] ^ state[a], 16)
        state[c] = (state[c] + state[d]) & 0xffffffff
        state[b] = rotate(state[b] ^ state[c], 12)
        state[a] = (state[a] + state[b]) & 0xffffffff
        state[d] = rotate(state[d] ^ state[a], 8)
        state[c] = (state[c] + state[d]) & 0xffffffff
        state[b] = rotate(state[b] ^ state[c], 7)

    for _ in range(10):
        for indices in [(0, 4, 8, 12), (1, 5, 9, 13), (2, 6, 10, 14),
                        (3, 7, 11, 15), (0, 5, 10, 15), (1, 6, 11, 12),
                        (2, 7, 8, 13), (3, 4, 9, 14)]:
            quarter(*indices)
    return struct.pack('<8I', *(state[i] for i in [0, 1, 2, 3, 12, 13, 14, 15]))


class NoiseIK:
    """Transcript independiente de Noise_IK_25519_ChaChaPoly_BLAKE2s."""
    def __init__(self, prologue, responder_public):
        name = b'Noise_IK_25519_ChaChaPoly_BLAKE2s'
        self.ck = digest(name) if len(name) > 32 else name.ljust(32, b'\0')
        self.h = self.ck
        self.mix_hash(prologue)
        self.mix_hash(responder_public)
        self.key = None
        self.nonce = 0

    def mix_hash(self, data):
        self.h = digest(self.h + data)

    @staticmethod
    def hkdf(ck, data):
        keyed = lambda key, value: hmac.new(key, value, hashlib.blake2s).digest()
        temp = keyed(ck, data)
        first = keyed(temp, b'\x01')
        return first, keyed(temp, first + b'\x02')

    def mix_key(self, data):
        self.ck, self.key = self.hkdf(self.ck, data)
        self.nonce = 0

    def encrypt_hash(self, data):
        nonce = bytes(4) + struct.pack('<Q', self.nonce)
        encrypted = ChaCha20Poly1305(self.key).encrypt(nonce, data, self.h)
        self.nonce += 1
        self.mix_hash(encrypted)
        return encrypted


def generate():
    sr = X25519PrivateKey.from_private_bytes(bytes([0x42]) * 32)
    responder = public(sr)
    prefix = bytes.fromhex('020101020304') + bytes(range(108))
    k_mac1 = digest(b'label-mac1' + responder)
    mac1 = mac(k_mac1, prefix)
    base = prefix + mac1
    values = dict(public=responder, k_mac1=k_mac1, prefix=prefix,
                  mac1=mac1, init_zero=base + bytes(16))
    sources = {
        'ipv4': bytes.fromhex('01c0000201ca6c'),
        'ipv6': bytes.fromhex('0220010db8000000000000000000000001ca6c'),
        'relay': b'\x03' + bytes([0x11]) * 16 + bytes([7]) * 16
                 + bytes([0x22]) * 16 + bytes([0x33]) * 16,
    }
    for name, source in sources.items():
        cookie = mac(bytes(range(32)), source)
        mac2 = mac(cookie, base)
        values.update({name + '_source': source, name + '_cookie': cookie,
                       name + '_mac2': mac2, name + '_init': base + mac2})
    nonce = bytes(range(24))
    k_reply = digest(b'label-cookie' + responder + mac1)
    header = bytes.fromhex('020401020304') + nonce
    aad = header + mac1
    cipher = ChaCha20Poly1305(hchacha20(k_reply, nonce[:16]))
    encrypted = cipher.encrypt(bytes(4) + nonce[16:], values['ipv4_cookie'], aad)
    assert cipher.decrypt(bytes(4) + nonce[16:], encrypted, aad) == values['ipv4_cookie']
    values.update(k_reply=k_reply, nonce=nonce, aad=aad,
                  encrypted_cookie=encrypted, cookie_reply=header + encrypted)

    si = X25519PrivateKey.from_private_bytes(bytes([0x41]) * 32)
    ei = X25519PrivateKey.from_private_bytes(bytes([0x43]) * 32)
    er = X25519PrivateKey.from_private_bytes(bytes([0x44]) * 32)
    noise = NoiseIK(b'atenea-one/v2' + bytes([7]) * 16, responder)
    # -> e, es, s, ss; payload índice y timestamp.
    noise.mix_hash(public(ei))
    noise.mix_key(ei.exchange(sr.public_key()))
    encrypted_static = noise.encrypt_hash(public(si))
    noise.mix_key(si.exchange(sr.public_key()))
    msg1 = public(ei) + encrypted_static + noise.encrypt_hash(
        struct.pack('>IQ', 0x01020304, 1700000000))
    # <- e, ee, se; payload índice del respondedor.
    noise.mix_hash(public(er))
    noise.mix_key(er.exchange(ei.public_key()))
    noise.mix_key(er.exchange(si.public_key()))
    msg2 = public(er) + noise.encrypt_hash(struct.pack('>I', 0x05060708))
    k1, k2 = noise.hkdf(noise.ck, b'')
    real_prefix = bytes.fromhex('020101020304') + msg1
    transport_i = ChaCha20Poly1305(k1).encrypt(bytes(12), b'iniciador', b'')
    transport_r = ChaCha20Poly1305(k2).encrypt(bytes(12), b'respondedor', b'')
    assert ChaCha20Poly1305(k1).decrypt(bytes(12), transport_i, b'') == b'iniciador'
    assert ChaCha20Poly1305(k2).decrypt(bytes(12), transport_r, b'') == b'respondedor'
    values.update(noise_msg1=msg1, noise_msg2=msg2,
                  real_init=real_prefix + mac(k_mac1, real_prefix) + bytes(16),
                  real_resp=bytes.fromhex('02020506070801020304') + msg2,
                  transport_i=transport_i, transport_r=transport_r)
    return values


def check_table(values, document):
    rows = re.findall(r'^\|\s*([^|]+?)\s*\|\s*`([0-9a-f]+)`\s*\|\s*$',
                      document.read_text(encoding='utf-8'), flags=re.MULTILINE)
    published = {}
    for label, encoded in rows:
        label = label.strip().strip('`')
        if label in published:
            raise ValueError(f'fila duplicada: {label}')
        published[label] = bytes.fromhex(encoded)
    failures = sorted(set(values) ^ set(published))
    failures += [label for label in values.keys() & published.keys()
                 if values[label] != published[label]]
    if failures:
        raise ValueError('vectores ausentes, adicionales o distintos: ' + ', '.join(failures))
    print(f'{document.name}: {len(values)} salidas hex coinciden')


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--check', action='store_true', help='comparar con la tabla publicada')
    args = parser.parse_args()
    values = generate()
    if args.check:
        check_table(values, Path(__file__).resolve().parents[2] / 'docs/protocol/wire-v2.md')
    else:
        print('| Salida | Hex |\n| --- | --- |')
        for name, value in values.items():
            print(f'| `{name}` | `{value.hex()}` |')


if __name__ == '__main__':
    try:
        main()
    except ValueError as error:
        sys.exit(str(error))
