#!/usr/bin/env python3
"""Utilidades del laboratorio (solo biblioteca estándar)."""
import hashlib
import json
import random
import socket
import struct
import subprocess
import sys
import time


def pcap_records(path):
    """Itera (src_ip, dst_ip, sport, dport, payload) de los UDP/IPv4 de un pcap clásico en Ethernet."""
    data = open(path, "rb").read()
    magic = struct.unpack("<I", data[:4])[0]
    if magic not in (0xA1B2C3D4, 0xA1B23C4D):
        raise SystemExit("formato pcap no soportado")
    off = 24
    while off + 16 <= len(data):
        _, _, caplen, _ = struct.unpack("<IIII", data[off : off + 16])
        pkt = data[off + 16 : off + 16 + caplen]
        off += 16 + caplen
        if len(pkt) < 42 or pkt[12:14] != b"\x08\x00" or pkt[23] != 17:
            continue
        ihl = (pkt[14] & 0x0F) * 4
        u = 14 + ihl
        sport, dport, ulen, _ = struct.unpack("!HHHH", pkt[u : u + 8])
        yield (
            socket.inet_ntoa(pkt[26:30]),
            socket.inet_ntoa(pkt[30:34]),
            sport,
            dport,
            pkt[u + 8 : u + ulen],
        )


def cmd_contains(pcap, marker):
    raw = open(pcap, "rb").read()
    print(raw.count(marker.encode()))


def cmd_data_datagram(pcap, src, dport, n):
    """Escribe en hex el n-ésimo datagrama de datos (tipo 3) de src hacia dport."""
    found = [p for s, _, _, d, p in pcap_records(pcap) if s == src and d == int(dport) and len(p) > 1 and p[1] == 3]
    if len(found) <= int(n):
        raise SystemExit(f"solo {len(found)} datagramas de datos")
    print(found[int(n)].hex())


def cmd_inject(dst, port, hexpayload, mode):
    """Reenvía un datagrama (replay) o una variante, desde un puerto efímero."""
    p = bytearray.fromhex(hexpayload)
    if mode == "bump-counter":  # contador fresco, etiqueta AEAD ya inválida
        c = struct.unpack("!Q", bytes(p[6:14]))[0] + 1_000_000
        p[6:14] = struct.pack("!Q", c)
    elif mode == "oversize":
        p = bytearray(random.Random(7).randbytes(2000))
    elif mode != "replay":
        raise SystemExit("modo desconocido")
    s = socket.socket(socket.AF_INET, socket.SOCK_DGRAM)
    s.sendto(bytes(p), (dst, int(port)))


def cmd_counter(sock, name):
    out = subprocess.check_output(["atenea", "status", "--socket", sock])
    print(json.loads(out)["counters"].get(name, 0))


def _payload(n, seed=1):
    return random.Random(seed).randbytes(n)


def cmd_tcp_server(bind, port):
    fam = socket.AF_INET6 if ":" in bind else socket.AF_INET
    s = socket.socket(fam, socket.SOCK_STREAM)
    s.setsockopt(socket.SOL_SOCKET, socket.SO_REUSEADDR, 1)
    s.bind((bind, int(port)))
    s.listen(1)
    s.settimeout(120)
    c, _ = s.accept()
    h, n = hashlib.sha256(), 0
    while True:
        b = c.recv(65536)
        if not b:
            break
        h.update(b)
        n += len(b)
    print(json.dumps({"bytes": n, "sha256": h.hexdigest()}))


def cmd_tcp_client(dst, port, nbytes, seed):
    fam = socket.AF_INET6 if ":" in dst else socket.AF_INET
    data = _payload(int(nbytes), int(seed))
    s = socket.socket(fam, socket.SOCK_STREAM)
    s.settimeout(120)
    for _ in range(50):  # el servidor puede tardar en estar listo
        try:
            s.connect((dst, int(port)))
            break
        except OSError:
            time.sleep(0.2)
    else:
        raise SystemExit("no conecta")
    t = time.time()
    s.sendall(data)
    s.shutdown(socket.SHUT_WR)
    s.recv(1)
    dt = max(time.time() - t, 1e-6)
    print(json.dumps({"bytes": len(data), "sha256": hashlib.sha256(data).hexdigest(), "mbit_s": round(len(data) * 8 / dt / 1e6, 1)}))


def cmd_udp_server(bind, port, seconds):
    fam = socket.AF_INET6 if ":" in bind else socket.AF_INET
    s = socket.socket(fam, socket.SOCK_DGRAM)
    s.bind((bind, int(port)))
    s.settimeout(float(seconds))
    got = set()
    try:
        while True:
            b, _ = s.recvfrom(4096)
            got.add(struct.unpack("!I", b[:4])[0])
    except socket.timeout:
        pass
    print(len(got))


def cmd_udp_client(dst, port, count):
    fam = socket.AF_INET6 if ":" in dst else socket.AF_INET
    s = socket.socket(fam, socket.SOCK_DGRAM)
    for i in range(int(count)):
        s.sendto(struct.pack("!I", i) + b"x" * 200, (dst, int(port)))
        time.sleep(0.002)


def cmd_send_udp(dst, port, text):
    s = socket.socket(socket.AF_INET, socket.SOCK_DGRAM)
    s.sendto(text.encode(), (dst, int(port)))


def cmd_field(sock, dotted):
    """Valor de un campo del estado del agente (ruta con puntos); 'none' si es null."""
    v = json.loads(subprocess.check_output(["atenea", "status", "--socket", sock]))
    for k in dotted.split("."):
        v = v[int(k)] if isinstance(v, list) else v[k]
    print("none" if v is None else str(v).lower() if isinstance(v, bool) else v)


def cmd_flip_last(path):
    """Altera el último byte de un fichero (la firma de una configuración)."""
    b = bytearray(open(path, "rb").read())
    b[-1] ^= 1
    open(path, "wb").write(bytes(b))


def cmd_spoof(src, dst, dport):
    """Envía un paquete UDP con IP de origen falsificada por la interfaz del túnel."""
    s = socket.socket(socket.AF_INET, socket.SOCK_RAW, socket.IPPROTO_RAW)
    payload = struct.pack("!HHHH", 4242, int(dport), 8, 0)
    ip = struct.pack("!BBHHHBBH4s4s", 0x45, 0, 20 + len(payload), 1, 0, 64, 17, 0, socket.inet_aton(src), socket.inet_aton(dst))
    s.sendto(ip + payload, (dst, 0))

if __name__ == "__main__":
    fn = globals().get("cmd_" + sys.argv[1].replace("-", "_"))
    if fn is None:
        raise SystemExit("orden desconocida")
    fn(*sys.argv[2:])
