#!/usr/bin/env python3
# Copyright 2026 Tutitoos
# SPDX-License-Identifier: Apache-2.0
"""Prueba de humo de la aplicación de escritorio (ADR-019): arranca el binario real con ATENEA_SMOKE=1, que abre
la ventana con la interfaz embebida y termina en cuanto la página llama a `NetworkList`. Si la CSP, el filtro del
runtime o el puente de Wails fallasen, la página no llegaría a llamar y el plazo vence.

Uso: smoke.py <segundos> <orden...>      (en Linux, dentro de `xvfb-run -a dbus-run-session --`)
Sale con 0 solo si la salida contiene SMOKE-OK."""
import os
import subprocess
import sys
import tempfile

timeout = int(sys.argv[1])
command = sys.argv[2:]
with tempfile.TemporaryDirectory() as scratch:
    env = dict(os.environ)
    env["ATENEA_SMOKE"] = "1"
    # Sin CLI instalado, `network list` es una lista vacía: no depende del equipo del runner.
    env["ATENEA_CLI"] = os.path.join(scratch, "no-existe")
    log = os.path.join(scratch, "salida.txt")
    with open(log, "wb") as out:
        proc = subprocess.Popen(command, stdout=out, stderr=subprocess.STDOUT, env=env)
        try:
            code = proc.wait(timeout=timeout)
        except subprocess.TimeoutExpired:
            proc.kill()
            proc.wait()
            code = "plazo agotado"
    text = open(log, "rb").read().decode("utf-8", "replace")
print(text)
print(f"salida del proceso: {code}")
ok = "SMOKE-OK" in text
print("PRUEBA DE HUMO:", "OK" if ok else "FALLO")
sys.exit(0 if ok else 1)
