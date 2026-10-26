#!/usr/bin/env python3
# Copyright 2026 Tutitoos
# SPDX-License-Identifier: Apache-2.0
"""Cliente mínimo del control plane para el laboratorio E2E (solo biblioteca estándar).

  cp.py login [usuario] [contraseña]      inicio de sesión OIDC real contra Dex; guarda cookies y CSRF
  cp.py api MÉTODO RUTA [--json TEXTO] [--file FICHERO] [--field a.b.c]
"""
import html
import http.cookiejar
import json
import os
import re
import sys
import urllib.error
import urllib.parse
import urllib.request

BASE = os.environ.get("CP_URL", "http://192.0.2.254:8080")
JAR = os.environ.get("CP_JAR", "/tmp/lab/cp.cookies")
CSRF_FILE = JAR + ".csrf"


def opener():
    jar = http.cookiejar.LWPCookieJar(JAR)
    if os.path.exists(JAR):
        jar.load(ignore_discard=True, ignore_expires=True)
    return jar, urllib.request.build_opener(urllib.request.HTTPCookieProcessor(jar))


def login(user="admin@example.test", password="password"):
    jar, op = opener()
    try:
        page = op.open(BASE + "/auth/login", timeout=15)
        body = page.read().decode()
        url = page.geturl()
    except urllib.error.HTTPError as e:
        raise SystemExit(f"login: HTTP {e.code} en {e.url}")
    m = re.search(r'<form[^>]*action="([^"]*)"', body)
    if not m:
        raise SystemExit("login: no se encontró el formulario de Dex")
    action = urllib.parse.urljoin(url, html.unescape(m.group(1)))
    data = urllib.parse.urlencode({"login": user, "password": password}).encode()
    try:
        op.open(urllib.request.Request(action, data=data), timeout=15).read()
    except urllib.error.HTTPError as e:
        if e.code not in (404, 405):  # el destino final tras el callback puede no existir
            raise SystemExit(f"login: HTTP {e.code} en {e.url}")
    if not any(c.name == "atenea_session" for c in jar):
        raise SystemExit("login: el control plane no creó la sesión")
    jar.save(ignore_discard=True, ignore_expires=True)
    status, text = call("GET", "/api/v1/session")
    open(CSRF_FILE, "w").write(json.loads(text)["csrf_token"])
    print(json.loads(text)["user"].get("email", "usuario"))


def call(method, path, body=None, content_type="application/json"):
    jar, op = opener()
    headers = {}
    if method not in ("GET", "HEAD") and os.path.exists(CSRF_FILE):
        headers["X-CSRF-Token"] = open(CSRF_FILE).read().strip()
    if body is not None:
        headers["Content-Type"] = content_type
    req = urllib.request.Request(BASE + path, data=body, method=method, headers=headers)
    try:
        with op.open(req, timeout=15) as r:
            return r.status, r.read().decode()
    except urllib.error.HTTPError as e:
        return e.code, e.read().decode()


def api(method, path, json_text=None, file=None, field=None):
    body, ctype = None, "application/json"
    if json_text is not None:
        body = json_text.encode()
    elif file is not None:
        body, ctype = open(file, "rb").read(), "application/octet-stream"
    status, text = call(method, path, body, ctype)
    if status >= 400:
        print(f"HTTP {status} {text}", file=sys.stderr)
        raise SystemExit(1)
    if field:
        v = json.loads(text)
        for k in field.split("."):
            v = v[int(k)] if isinstance(v, list) else v[k]
        print(json.dumps(v) if isinstance(v, (dict, list, bool)) else v)
    else:
        print(text)


if __name__ == "__main__":
    cmd = sys.argv[1]
    if cmd == "login":
        login(*sys.argv[2:4])
    elif cmd == "api":
        a = sys.argv[4:]
        opts = {"json_text": None, "file": None, "field": None}
        while a:
            flag, val = a[0], a[1]
            opts[{"--json": "json_text", "--file": "file", "--field": "field"}[flag]] = val
            a = a[2:]
        api(sys.argv[2], sys.argv[3], **opts)
    else:
        raise SystemExit("orden desconocida")
