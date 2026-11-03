#!/usr/bin/env python3
# Copyright 2026 Tutitoos
# SPDX-License-Identifier: Apache-2.0
"""Lista los ficheros de un directorio con tamaño y SHA-256 (spike S5). Uso: lsbin.py <dir>"""
import hashlib
import os
import sys

root = sys.argv[1]
if not os.path.isdir(root):
    print(f"{root}: no existe")
    sys.exit(0)
for dirpath, _dirs, files in sorted(os.walk(root)):
    for name in sorted(files):
        path = os.path.join(dirpath, name)
        digest = hashlib.sha256(open(path, "rb").read()).hexdigest()
        print(f"{os.path.getsize(path):>12}  {digest}  {os.path.relpath(path, root)}")
