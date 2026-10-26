#!/usr/bin/env bash
# Copyright 2026 Tutitoos
# SPDX-License-Identifier: Apache-2.0
set -euo pipefail
if [ "$#" -gt 1 ]; then
    echo "Uso: $0 [DIRECTORIO_SNAPSHOT]" >&2
    exit 2
fi
root=$(cd "$(dirname "$0")/../.." && pwd)
export PYTHONDONTWRITEBYTECODE=1
exec python3 - "${1:-$root/dist/audit}" <<'PY'
import gzip
import hashlib
import json
from pathlib import Path
import re
import sys
import tarfile

folder = Path(sys.argv[1])
def require(condition, message):
    if not condition:
        raise SystemExit(message)

expected = {'COMMIT', 'SOURCE_DATE_EPOCH', 'dependencies.json', 'source.tar.gz'}
lines = (folder / 'SHA256SUMS').read_text().splitlines()
seen = set()
for line in lines:
    match = re.fullmatch(r'([0-9a-f]{64})  ([A-Za-z0-9_.-]+)', line)
    require(match is not None, 'SHA256SUMS mal formado')
    digest, name = match.groups()
    require(name in expected and name not in seen, 'Entrada inesperada o repetida')
    require(hashlib.sha256((folder / name).read_bytes()).hexdigest() == digest,
            'SHA-256 incorrecto: ' + name)
    seen.add(name)
require(seen == expected, 'Faltan entradas en SHA256SUMS')
commit = (folder / 'COMMIT').read_text().strip()
require(re.fullmatch(r'[0-9a-f]{40}', commit) is not None, 'Commit mal formado')
epoch = int((folder / 'SOURCE_DATE_EPOCH').read_text().strip())
prefix = 'atenea-one-' + commit + '/'
with (folder / 'source.tar.gz').open('rb') as compressed:
    header = compressed.read(10)
require(header[:4] == b'\x1f\x8b\x08\x00' and int.from_bytes(header[4:8], 'little') == epoch,
        'Metadatos gzip inesperados')
with tarfile.open(folder / 'source.tar.gz', 'r:gz') as archive:
    members = archive.getmembers()
    names = [m.name for m in members]
    require(names == sorted(set(names)), 'Orden o nombres duplicados incorrectos')
    for member in members:
        path = Path(member.name)
        require(member.name.startswith(prefix) and '..' not in path.parts and not path.is_absolute(),
                'Ruta fuera del snapshot')
        require(member.isfile() or member.isdir(), 'Tipo no admitido')
        require(member.uid == member.gid == 0 and member.uname == member.gname == ''
                and member.mtime == epoch and not member.pax_headers,
                'Metadatos tar no normalizados')
        require(member.mode in ({0o755} if member.isdir() else {0o644, 0o755}),
                'Modo inesperado')
    provenance = json.load(archive.extractfile(prefix + 'AUDIT-SNAPSHOT.json'))
    require(provenance == {'commit': commit, 'source_date_epoch': epoch,
                           'format': 'atenea-audit-snapshot/1'}, 'Procedencia incoherente')
    require(archive.extractfile(prefix + 'AUDIT-DEPENDENCIES.json').read()
            == (folder / 'dependencies.json').read_bytes(), 'Dependencias incoherentes')
    require(json.loads((folder / 'dependencies.json').read_bytes())['commit'] == commit,
            'Commit de dependencias incoherente')
print('OK: hashes, procedencia, dependencias, orden y metadatos normalizados (' + commit + ')')
print('Integridad local; SHA256SUMS no es una firma ni prueba de autoría.')
PY
