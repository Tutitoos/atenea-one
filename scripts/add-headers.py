#!/usr/bin/env python3
# Copyright 2026 Tutitoos
# SPDX-License-Identifier: Apache-2.0
"""Añade la cabecera de copyright a los ficheros fuente autorados. Idempotente.

Uso: scripts/add-headers.py [ruta ...]   (sin rutas: todos los ficheros del repositorio)
Solo toca las extensiones de PREFIX; no toca generados, migraciones SQL, JSON/TOML/YAML/MD,
fixtures ni semillas. Un shebang se conserva como primera línea.
"""
import os
import subprocess
import sys

TITULAR = "Copyright 2026 Tutitoos"
SPDX = "SPDX-License-Identifier: Apache-2.0"
PREFIX = {
    ".rs": "//", ".ts": "//", ".tsx": "//", ".mjs": "//", ".js": "//",
    ".py": "#", ".sh": "#", ".ps1": "#", ".swift": "//",
}
EXCLUIDOS = {"node_modules", "target", "dist", "build", "vendor", "__pycache__"}
RAIZ = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))


def prefijo(ruta):
    """Prefijo de comentario si el fichero lleva cabecera; None si no aplica."""
    partes = ruta.split("/")
    if EXCLUIDOS.intersection(partes) or ruta.startswith("tests/load/out/"):
        return None
    ext = os.path.splitext(ruta)[1]
    if ext:
        return PREFIX.get(ext)
    # Sin extensión (p. ej. packaging/deb/postinst): solo scripts de shell por shebang.
    try:
        with open(os.path.join(RAIZ, ruta), "rb") as f:
            primera = f.readline()
    except OSError:
        return None
    return "#" if primera.startswith(b"#!") and b"sh" in primera else None


def ficheros():
    salida = subprocess.check_output(
        ["git", "ls-files", "-z", "--cached", "--others", "--exclude-standard"], cwd=RAIZ
    )
    return [r for r in salida.decode().split("\0") if r and prefijo(r)]


def cabecera(pref):
    return [f"{pref} {TITULAR}", f"{pref} {SPDX}"]


def tiene_cabecera(texto, pref):
    lineas = texto.splitlines()
    if lineas and lineas[0].startswith("#!"):
        lineas = lineas[1:]
    return lineas[:2] == cabecera(pref)


def aplicar(ruta):
    """True si modificó el fichero."""
    pref = prefijo(ruta)
    destino = os.path.join(RAIZ, ruta)
    with open(destino, encoding="utf-8", newline="") as f:
        texto = f.read()
    if tiene_cabecera(texto, pref):
        return False
    nl = "\r\n" if "\r\n" in texto else "\n"
    bloque = nl.join(cabecera(pref)) + nl
    if texto.startswith("#!"):
        fin = texto.find("\n") + 1
        if fin == 0:  # shebang sin salto de línea final
            texto, fin = texto + nl, len(texto) + len(nl)
        nuevo = texto[:fin] + bloque + texto[fin:]
    else:
        nuevo = bloque + texto
    with open(destino, "w", encoding="utf-8", newline="") as f:
        f.write(nuevo)
    return True


def main(argv):
    rutas = [os.path.relpath(os.path.abspath(a), RAIZ) for a in argv] or ficheros()
    cambiados = [r for r in rutas if prefijo(r) and aplicar(r)]
    for r in cambiados:
        print(f"cabecera añadida: {r}")
    print(f"{len(cambiados)} fichero(s) modificados")


if __name__ == "__main__":
    main(sys.argv[1:])
