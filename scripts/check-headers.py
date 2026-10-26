#!/usr/bin/env python3
# Copyright 2026 Tutitoos
# SPDX-License-Identifier: Apache-2.0
"""Falla si un fichero fuente autorado no lleva la cabecera de copyright.

`--fix` la añade con scripts/add-headers.py (útil para ficheros nuevos).
"""
import importlib.util
import os
import sys

spec = importlib.util.spec_from_file_location(
    "add_headers", os.path.join(os.path.dirname(os.path.abspath(__file__)), "add-headers.py")
)
add = importlib.util.module_from_spec(spec)
spec.loader.exec_module(add)


def faltan():
    malos = []
    for ruta in add.ficheros():
        with open(os.path.join(add.RAIZ, ruta), encoding="utf-8", newline="") as f:
            if not add.tiene_cabecera(f.read(), add.prefijo(ruta)):
                malos.append(ruta)
    return malos


def main():
    malos = faltan()
    if malos and "--fix" in sys.argv[1:]:
        add.main(malos)
        malos = faltan()
    for ruta in malos:
        print(f"sin cabecera o distinta: {ruta}", file=sys.stderr)
    if malos:
        sys.exit(f"{len(malos)} fichero(s); ejecute scripts/check-headers.py --fix")
    print("cabeceras: correctas")


if __name__ == "__main__":
    main()
