#!/usr/bin/env python3
# Copyright 2026 Tutitoos
# SPDX-License-Identifier: Apache-2.0
"""Cada mutación conserva el resto del release; la copia original se restaura."""
import json
import subprocess
import sys
from pathlib import Path

root, dist, public, other, commit, version = sys.argv[1:]
dist = Path(dist)
manifest = json.loads((dist / 'release-manifest.json').read_text())


def reject(label, key=public, expected=commit):
    result = subprocess.run(['bash', root + '/scripts/release/verify.sh', str(dist), key, expected, version], stdout=subprocess.PIPE, stderr=subprocess.STDOUT)
    assert result.returncode != 0, label + ': se aceptó la alteración'
    print('PASS rechazo: ' + label)


def mutate(name, label, transform):
    path = dist / name
    original = path.read_bytes()
    try:
        path.write_bytes(transform(original))
        reject(label)
    finally:
        path.write_bytes(original)


mutate('atenea-linux', 'un byte distinto en artefacto', lambda b: bytes([b[0] ^ 1]) + b[1:])
mutate(manifest['sboms'][0]['name'], 'SBOM alterado', lambda b: b + b' ')
reject('firma de otra clave', key=other)
reject('commit distinto', expected='0' * 40 if commit != '0' * 40 else '1' * 40)
mutate('release-manifest.json', 'manifiesto truncado', lambda b: b[:len(b)//2])
