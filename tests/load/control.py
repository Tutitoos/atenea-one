#!/usr/bin/env python3
# Copyright 2026 Tutitoos
# SPDX-License-Identifier: Apache-2.0
"""Carga real aislada; biblioteca estándar, firmas mediante atenea-load-sign."""
import argparse
import base64
import concurrent.futures
import hashlib
import http.client
import json
import math
import os
from pathlib import Path
import secrets
import socket
import ssl
import struct
import subprocess
import threading
import time
import urllib.parse
import uuid

LOCK = threading.Lock()
LOCAL = threading.local()
SIGNERS = []
SAMPLES = []

def pg_env():
    value=urllib.parse.urlsplit(os.environ['DATABASE_URL'])
    return dict(os.environ,PGHOST=value.hostname,PGPORT=str(value.port or 5432),
                PGUSER=urllib.parse.unquote(value.username or 'postgres'),PGPASSWORD=urllib.parse.unquote(value.password or ''),
                PGDATABASE=value.path.lstrip('/'))
PHASE_WINDOWS = {}


def emit(**row):
    with LOCK:
        print(json.dumps(dict(at=time.time(), **row), separators=(",", ":")), flush=True)


def devices(directory):
    keys = "network device seed bundle noise identity peer ordinal".split()
    rows = [dict(zip(keys, line.split("\t"))) for line in (directory / "devices.tsv").read_text().splitlines()]
    assert rows and all(len(r) == 8 for r in rows)
    # Intercalar redes evita que la precarga sature una sola cuota de red.
    return sorted(rows, key=lambda r: (int(r["ordinal"]), r["network"]))


def authorization(args, d, method, path, body):
    if not hasattr(LOCAL, "signer"):
        LOCAL.signer = subprocess.Popen([args.signer, "sign"], stdin=subprocess.PIPE, stdout=subprocess.PIPE, text=True)
        with LOCK:
            SIGNERS.append(LOCAL.signer)
    proc = LOCAL.signer
    proc.stdin.write("\t".join([d["seed"], d["device"], method, path, str(int(time.time())), secrets.token_hex(16), body.hex()]) + "\n")
    proc.stdin.flush()
    header = proc.stdout.readline().strip()
    if not header.startswith("AtDevice v1 "):
        raise RuntimeError("El proceso de firma terminó sin cabecera")
    return header


def context(args):
    return ssl.create_default_context(cafile=args.ca) if args.ca else ssl.create_default_context()


def request(args, phase, path, d=None, body=None, method="GET", expected=(200,)):
    payload = b"" if body is None else json.dumps(body, separators=(",", ":")).encode()
    start = time.monotonic()
    status, data, error = 0, b"", None
    conn = None
    try:
        url = urllib.parse.urlsplit(args.url)
        headers = {"Content-Type": "application/json"}
        if d:
            headers["Authorization"] = authorization(args, d, method, path, payload)
        else:
            headers["Cookie"] = "atenea_session=" + args.session
            headers["X-CSRF-Token"] = args.csrf
        cls = http.client.HTTPSConnection if url.scheme == "https" else http.client.HTTPConnection
        kw = {"context": context(args)} if url.scheme == "https" else {}
        conn = cls(url.hostname, url.port, timeout=args.timeout, **kw)
        conn.request(method, path, body=payload if body is not None else None, headers=headers)
        response = conn.getresponse()
        status, data = response.status, response.read()
    except (OSError, ValueError, http.client.HTTPException, RuntimeError) as exc:
        error = type(exc).__name__ + ": " + str(exc)
    finally:
        if conn:
            conn.close()
    finished = time.monotonic()
    ms = (finished - start) * 1000
    row = dict(kind="request", phase=phase, status=status, ok=status in expected, latency_ms=ms, bytes=len(data))
    if error:
        row["error"] = error
    if status >= 400:
        row["response"] = data[:256].decode(errors="replace")
    emit(**row)
    with LOCK:
        SAMPLES.append((phase, status, ms, status in expected))
        begin, end = PHASE_WINDOWS.get(phase, (start, finished))
        PHASE_WINDOWS[phase] = (min(begin, start), max(end, finished))
    return status, data


def batch(args, jobs, rate=0):
    """Cola acotada: como máximo concurrency tareas pendientes."""
    with concurrent.futures.ThreadPoolExecutor(max_workers=args.concurrency) as pool:
        pending = set()
        target = time.monotonic()
        for job in jobs:
            if rate:
                time.sleep(max(0, target - time.monotonic()))
                target += 1 / rate
            pending.add(pool.submit(job))
            if len(pending) >= args.concurrency:
                done, pending = concurrent.futures.wait(pending, return_when=concurrent.futures.FIRST_COMPLETED)
                for future in done:
                    future.result()
        for future in concurrent.futures.as_completed(pending):
            future.result()


def announce(args, d):
    config = base64.b64encode((args.fixtures / (d["device"] + ".config")).read_bytes()).decode()
    return request(args, "announce", "/v1/announce", d, {"config": config, "candidates": ["198.51.100.10:51820"]}, "POST")


def coordinator(args, rows):
    rate = len({d["network"] for d in rows}) * 5
    batch(args, (lambda d=d: announce(args, d) for d in rows), rate)
    request(args, "presence", "/healthz")
    stop = threading.Event()
    def refresh():
        while not stop.wait(20):
            batch(args, (lambda d=d: announce(args, d) for d in rows), rate)
    keeper = threading.Thread(target=refresh)
    keeper.start()
    try:
        if args.quota:
            quotas(args, rows)
        deadline = time.monotonic() + args.duration
        index = 0
        def jobs():
            nonlocal index
            while time.monotonic() < deadline:
                d = rows[index % len(rows)]
                index += 1
                if args.quota:
                    d = rows[0]
                if index % 2:
                    yield lambda d=d: request(args, "rendezvous-quota" if args.quota else "rendezvous", "/v1/rendezvous", d, {"peer": d["peer"]}, "POST", (200, 429) if args.quota else (200,))
                else:
                    yield lambda d=d: request(args, "poll-quota" if args.quota else "poll", "/v1/events?wait=" + str(args.wait), d, expected=(200, 429) if args.quota else (200,))
        batch(args, jobs(), args.rate)
    finally:
        stop.set()
        keeper.join()
    status, raw = request(args, "presence-final", "/healthz")
    if status == 200:
        observed = json.loads(raw)["presence"]
        emit(kind="presence", requested=len(rows), observed=observed, complete=observed == len(rows))
        assert observed == len(rows), "No se sostuvo toda la población solicitada"


def quotas(args, rows):
    """Excesos firmados, aislamiento de otra red y liberación tras respuesta."""
    first = rows[0]
    other = next(d for d in rows if d["network"] != first["network"])
    # El último announce de este dispositivo fue al principio de la precarga.
    assert announce(args, first)[0] == 200
    config = base64.b64encode((args.fixtures / (first["device"] + ".config")).read_bytes()).decode()
    request(args, "announce-quota", "/v1/announce", first, {"config": config, "candidates": []}, "POST", (429,))
    with concurrent.futures.ThreadPoolExecutor(max_workers=2) as pool:
        held = [pool.submit(request, args, "poll-held-device", "/v1/events?wait=3", first) for _ in range(2)]
        time.sleep(.5)
        request(args, "poll-device-quota", "/v1/events?wait=0", first, expected=(429,))
        request(args, "poll-isolation", "/v1/events?wait=0", other)
        for future in held:
            future.result()
    request(args, "poll-recovered-device", "/v1/events?wait=0", first)
    same = [d for d in rows if d["network"] == first["network"]]
    capacity = math.ceil(args.poll_capacity / 4)
    if len(same) <= capacity:
        raise ValueError("La prueba de cuota de red necesita más de ceil(poll-capacity/4) dispositivos/red")
    with concurrent.futures.ThreadPoolExecutor(max_workers=capacity) as pool:
        held = []
        for d in same[:capacity]:
            held.append(pool.submit(request, args, "poll-held-network", "/v1/events?wait=30", d))
            time.sleep(.2)
        request(args, "poll-network-quota", "/v1/events?wait=0", same[capacity], expected=(429,))
        request(args, "poll-network-isolation", "/v1/events?wait=0", other)
        for future in held:
            future.result()
    request(args, "poll-recovered-network", "/v1/events?wait=0", same[capacity])


def cp(args, rows):
    end = time.monotonic() + args.duration
    index = 0
    since = urllib.parse.quote(time.strftime("%Y-%m-%dT%H:%M:%SZ", time.gmtime(time.time() - 7*86400)))
    def jobs():
        nonlocal index
        while time.monotonic() < end:
            d = rows[index % len(rows)]
            network = str(uuid.UUID(d["network"]))
            i = index
            index += 1
            if args.workload == "issuance" or (args.workload == "mixed" and i % 4 == 0):
                yield lambda d=d: request(args, "issuance", "/api/v1/device/config", d)
            else:
                suffix = ("devices", "metrics?since=" + since + "&limit=1000", "audit?limit=100")[i % 3]
                yield lambda p="/api/v1/networks/" + network + "/" + suffix: request(args, "read", p)
    batch(args, jobs(), args.rate)
    db_size(rows, "after-workload")


def sql_seed(args, rows):
    """Una transacción explícita contra una BD desechable ya migrada; nunca producción."""
    assert args.disposable, "seed exige --disposable"
    org, user = str(uuid.uuid4()), str(uuid.uuid4())
    session, csrf = secrets.token_hex(32), secrets.token_hex(32)
    sha = lambda s: hashlib.sha256(s.encode()).hexdigest()
    root = (args.fixtures / "root.pub").read_text().strip()
    statements = ["BEGIN;", f"INSERT INTO organizations(id,name) VALUES('{org}','load-control');",
        f"INSERT INTO users(id,oidc_issuer,oidc_subject,email) VALUES('{user}','load-fixture','{user}','load@example.test');",
        f"INSERT INTO memberships VALUES('{org}','{user}','owner');",
        f"INSERT INTO sessions(id,user_id,csrf_hash,csrf_token,expires_at) VALUES(decode('{sha(session)}','hex'),'{user}',decode('{sha(csrf)}','hex'),'{csrf}',now()+interval '8 hours');"]
    sessions = [session]
    for i in range(7):
        ws_user, token = str(uuid.uuid4()), secrets.token_hex(32)
        sessions.append(token)
        statements.extend([
            f"INSERT INTO users(id,oidc_issuer,oidc_subject) VALUES('{ws_user}','load-fixture','{ws_user}');",
            f"INSERT INTO memberships VALUES('{org}','{ws_user}','member');",
            f"INSERT INTO sessions(id,user_id,csrf_hash,csrf_token,expires_at) VALUES(decode('{sha(token)}','hex'),'{ws_user}',decode('{sha(csrf)}','hex'),'{csrf}',now()+interval '8 hours');",
        ])
    networks = sorted({d["network"] for d in rows})
    for n in networks:
        net = str(uuid.UUID(n))
        cert = (args.fixtures / (n + ".cert")).read_bytes().hex()
        statements.append(f"INSERT INTO networks(id,org_id,name,ipv4_cidr,ipv6_cidr,root_anchor,issuer_cert) VALUES('{net}','{org}','load-{n}','10.90.0.0/16','fd90::/64',decode('{root}','hex'),decode('{cert}','hex'));")
        statements.append(f"INSERT INTO access_policies(id,network_id,src,dst,proto,port_min,port_max,created_by) VALUES('{uuid.uuid4()}','{net}','{{\"kind\":\"any\"}}','{{\"kind\":\"any\"}}',0,0,0,'{user}');")
        # 10080 puntos/red, 7 días a 60 s: igual a la cadencia real del sampler.
        statements.append(f"INSERT INTO network_samples SELECT '{net}'::uuid, date_trunc('minute',now())-i*interval '1 minute',0,0,0,0,0,0 FROM generate_series(0,10079) i;")
        statements.append(f"INSERT INTO audit_events(at,actor_kind,actor_id,org_id,network_id,action,target) SELECT now()-i*interval '1 minute','system','load','{org}','{net}','load.synthetic','load' FROM generate_series(0,{args.events-1}) i;")
    for d in rows:
        dev, net = str(uuid.UUID(d["device"])), str(uuid.UUID(d["network"]))
        ordinal = int(d["ordinal"])
        ipv4 = f"10.90.{ordinal//256}.{ordinal%256}"
        statements.append(f"INSERT INTO devices(id,network_id,name,status,noise_pubkey,identity_pubkey,bundle,approved_at) VALUES('{dev}','{net}','load-{dev}','approved',decode('{d['noise']}','hex'),decode('{d['identity']}','hex'),decode('{d['bundle']}','hex'),now());")
        statements.append(f"INSERT INTO ip_allocations VALUES('{net}','{dev}',4,'{ipv4}'),('{net}','{dev}',6,'fd90::{ordinal:x}');")
        statements.append(f"INSERT INTO device_reports VALUES('{dev}',now(),'independent','[]');")
    # Reemplaza ceros de fixture por el número real de dispositivos de cada red.
    statements.append(f"UPDATE network_samples s SET total=(SELECT count(*) FROM devices d WHERE d.network_id=s.network_id),unknown=(SELECT count(*) FROM devices d WHERE d.network_id=s.network_id) WHERE network_id IN (SELECT id FROM networks WHERE org_id='{org}');")
    statements.extend(["COMMIT;", "ANALYZE;"])
    subprocess.run(["psql", "-X", "--set", "ON_ERROR_STOP=1"], env=pg_env(), input="\n".join(statements), text=True, stdout=subprocess.DEVNULL, check=True)
    credentials = args.fixtures / "session.json"
    credentials.write_text(json.dumps(dict(session=session, csrf=csrf, org=org, sessions=sessions)))
    credentials.chmod(0o600)
    emit(kind="seed", devices=len(rows), networks=len(networks), metric_points=len(networks)*10080, audit_events=len(networks)*args.events, synthetic=True)
    db_size(rows, "after-seed")


def db_size(rows, stage):
    query = "SELECT pg_database_size(current_database()), pg_total_relation_size('network_samples'), pg_total_relation_size('devices'), pg_total_relation_size('audit_events')"
    result = subprocess.run(["psql", "-X", "-At", "--set", "ON_ERROR_STOP=1", "-c", query],
        env=pg_env(), capture_output=True, text=True, check=True)
    total, metrics, inventory, audit = map(int, result.stdout.strip().split("|"))
    emit(kind="db-size", stage=stage, devices=len(rows), physical_bytes=total, network_samples_bytes=metrics, devices_bytes=inventory, audit_events_bytes=audit)


def cleanup(args, rows):
    assert args.disposable, "cleanup exige --disposable"
    org = str(uuid.UUID(json.loads((args.fixtures / "session.json").read_text())["org"]))
    sql = f"""BEGIN;
CREATE TEMP TABLE load_users AS SELECT user_id FROM memberships WHERE org_id='{org}';
DELETE FROM audit_events WHERE org_id='{org}';
DELETE FROM organizations WHERE id='{org}';
DELETE FROM users WHERE id IN (SELECT user_id FROM load_users);
COMMIT;"""
    subprocess.run(["psql", "-X", "--set", "ON_ERROR_STOP=1"], env=pg_env(), input=sql, text=True, stdout=subprocess.DEVNULL, check=True)
    emit(kind="cleanup", org=org)


def exact(sock, size):
    out = bytearray()
    while len(out) < size:
        part = sock.recv(size-len(out))
        if not part:
            raise EOFError("WebSocket cerrado")
        out.extend(part)
    return bytes(out)


def ws_frame(sock):
    first, second = exact(sock, 2)
    if second & 128:
        raise ValueError("Servidor envió trama enmascarada")
    size = second & 127
    if size == 126:
        size = struct.unpack("!H", exact(sock, 2))[0]
    elif size == 127:
        size = struct.unpack("!Q", exact(sock, 8))[0]
    if size > 1_048_576:
        raise ValueError("Trama excesiva")
    return first & 15, exact(sock, size)


def ws_send(sock, opcode, payload=b""):
    assert len(payload) <= 125
    mask = secrets.token_bytes(4)
    sock.sendall(bytes([128 | opcode, 128 | len(payload)]) + mask + bytes(v ^ mask[i%4] for i, v in enumerate(payload)))


def websocket(args, rows):
    url = urllib.parse.urlsplit(args.url)
    net = str(uuid.UUID(rows[0]["network"]))
    stop = time.monotonic() + args.duration
    upgrades, changed, failures, active = [], [], [], []
    def one(index):
        started = time.monotonic()
        sock = None
        try:
            sock = socket.create_connection((url.hostname, url.port or (443 if url.scheme == "https" else 80)), args.timeout)
            if url.scheme == "https":
                sock = context(args).wrap_socket(sock, server_hostname=url.hostname)
            key = base64.b64encode(secrets.token_bytes(16)).decode()
            path = f"/api/v1/networks/{net}/events"
            cookie = args.sessions[(index // 8) % len(args.sessions)]
            header = f"GET {path} HTTP/1.1\r\nHost: {url.netloc}\r\nOrigin: {args.origin or args.url.rstrip('/')}\r\nUpgrade: websocket\r\nConnection: Upgrade\r\nSec-WebSocket-Key: {key}\r\nSec-WebSocket-Version: 13\r\nCookie: atenea_session={cookie}\r\n\r\n"
            sock.sendall(header.encode())
            response = bytearray()
            while not response.endswith(b"\r\n\r\n"):
                response.extend(exact(sock, 1))
                if len(response) > 16384:
                    raise ValueError("Upgrade excesivo")
            status = int(response.split(b" ", 2)[1])
            with LOCK:
                upgrades.append(status)
            expected = base64.b64encode(hashlib.sha1((key + "258EAFA5-E914-47DA-95CA-C5AB0DC85B11").encode()).digest())
            if status != 101:
                emit(kind="websocket-upgrade", status=status, latency_ms=(time.monotonic()-started)*1000)
                return
            fields = {name.strip().lower(): value.strip() for name, value in (line.split(b":", 1) for line in bytes(response).split(b"\r\n")[1:] if b":" in line)}
            assert fields[b"sec-websocket-accept"].strip() == expected
            with LOCK:
                active.append(index)
            emit(kind="websocket-upgrade", status=101, latency_ms=(time.monotonic()-started)*1000)
            sock.settimeout(1)
            while time.monotonic() < stop:
                # No timeout a media trama: el timeout solo vigila disponibilidad inicial.
                import select
                if not select.select([sock], [], [], min(1, max(0, stop-time.monotonic())))[0]:
                    continue
                sock.settimeout(args.timeout)
                opcode, payload = ws_frame(sock)
                if opcode == 8:
                    emit(kind="websocket-close", bytes=len(payload))
                    break
                if opcode == 9:
                    ws_send(sock, 10, payload)
                elif opcode == 1:
                    event = json.loads(payload)
                    if event.get("type") == "changed":
                        with LOCK:
                            changed.append((index, event.get("seq")))
                    emit(kind="websocket-event", connection=index, event_type=event.get("type"), scope=event.get("scope"), seq=event.get("seq"))
            ws_send(sock, 8)
        except (OSError, EOFError, ValueError, AssertionError) as exc:
            emit(kind="websocket-error", error=type(exc).__name__ + ": " + str(exc))
            with LOCK:
                failures.append(str(exc))
        finally:
            if sock:
                sock.close()
    # Mantiene N conexiones mientras genera cambios reales de status por firma.
    with concurrent.futures.ThreadPoolExecutor(max_workers=args.connections) as pool:
        readers = [pool.submit(one, index) for index in range(args.connections)]
        time.sleep(1)
        i = 0
        while time.monotonic() < stop:
            request(args, "websocket-trigger", "/api/v1/device/status", rows[i % len(rows)], {"paths": []}, "POST", (204,))
            i += 1
            time.sleep(5.1)
        for future in readers:
            future.result()
    accepted = upgrades.count(101)
    emit(kind="websocket-summary", requested=args.connections, accepted=accepted, rejected=upgrades.count(429), changed_events=len(changed), receivers=len({i for i, _ in changed}), errors=len(failures))
    db_size(rows, "after-websocket")
    assert not failures and all(s in (101, 429) for s in upgrades), "Falló WebSocket"
    assert accepted == min(args.connections, len(args.sessions)*8, 64), "No se alcanzó la capacidad WebSocket esperada"
    assert active and set(active) == {i for i, _ in changed}, "Alguna conexión admitida no recibió cambios reales"


def summary():
    for phase in sorted({r[0] for r in SAMPLES}):
        rows = [r for r in SAMPLES if r[0] == phase]
        values = sorted(r[2] for r in rows)
        percentile = lambda p: values[max(0, math.ceil(len(values)*p)-1)]
        begin, end = PHASE_WINDOWS[phase]
        wall = max(end - begin, 1e-9)
        successes = sum(200 <= r[1] < 300 for r in rows)
        emit(kind="summary", phase=phase, requests=len(rows), errors=sum(not r[3] for r in rows), statuses={str(s): sum(r[1] == s for r in rows) for s in sorted({r[1] for r in rows})}, wall_s=wall, successes=successes, success_rps=successes/wall, request_rps=len(rows)/wall, p50_ms=percentile(.5), p95_ms=percentile(.95), p99_ms=percentile(.99))


def main():
    p = argparse.ArgumentParser(description=__doc__)
    p.add_argument("mode", choices=["seed", "cleanup", "coordinator", "control-plane", "websocket", "self-check"])
    p.add_argument("--fixtures", type=Path, default=Path("/results/fixtures"))
    p.add_argument("--signer", default="atenea-load-sign")
    p.add_argument("--url", default="http://coordinator:8081")
    p.add_argument("--ca")
    p.add_argument("--concurrency", type=int, choices=[1, 50, 200], default=50)
    p.add_argument("--duration", type=float, default=10)
    p.add_argument("--timeout", type=float, default=40)
    p.add_argument("--rate", type=float, default=0)
    p.add_argument("--wait", type=int, choices=range(31), default=1)
    p.add_argument("--poll-capacity", type=int, default=256)
    p.add_argument("--quota", action="store_true")
    p.add_argument("--workload", choices=["read", "issuance", "mixed"], default="mixed")
    p.add_argument("--session-file", type=Path)
    p.add_argument("--connections", type=int, default=8)
    p.add_argument("--origin")
    p.add_argument("--events", type=int, default=10080)
    p.add_argument("--disposable", action="store_true")
    args = p.parse_args()
    if not all(math.isfinite(n) for n in (args.duration, args.timeout, args.rate)) or args.duration <= 0 or args.timeout <= 0 or args.rate < 0 or args.events < 1 or args.connections < 1 or args.poll_capacity < 1:
        p.error("duración/timeout/eventos/conexiones/capacidad deben ser positivos finitos; rate >= 0")
    args.session, args.csrf = "", ""
    if args.mode == "self-check":
        a, b = socket.socketpair()
        a.sendall(b'\x81\x02{}')
        assert ws_frame(b) == (1, b'{}')
        ws_send(a, 10, b"ok")
        header, mask, payload = exact(b, 2), exact(b, 4), exact(b, 2)
        assert header == b'\x8a\x82' and bytes(v ^ mask[i%4] for i, v in enumerate(payload)) == b"ok"
        a.close()
        b.close()
        emit(kind="self-check", ok=True)
        return
    rows = devices(args.fixtures)
    if args.mode in ("control-plane", "websocket"):
        credentials = json.loads((args.session_file or args.fixtures / "session.json").read_text())
        args.session, args.csrf = credentials["session"], credentials["csrf"]
        args.sessions = credentials.get("sessions", [args.session])
    emit(kind="run", mode=args.mode, devices=len(rows), networks=len({r['network'] for r in rows}), concurrency=args.concurrency, duration=args.duration, quota=args.quota, connections=args.connections, workload=args.workload, rate=args.rate)
    try:
        {"seed": sql_seed, "cleanup": cleanup, "coordinator": coordinator, "control-plane": cp, "websocket": websocket}[args.mode](args, rows)
    finally:
        summary()
        for proc in SIGNERS:
            proc.stdin.close()
            proc.wait(timeout=10)
    # Los excesos esperados se conservan en statuses; otros errores hacen fallar el proceso.
    if any(not r[3] for r in SAMPLES):
        raise SystemExit(1)


if __name__ == "__main__":
    main()
