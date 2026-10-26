#!/usr/bin/env bash
set -euo pipefail
if [ "$#" -ne 0 ]; then
    echo "Uso: $0 (archiva HEAD, no el árbol de trabajo)" >&2
    exit 2
fi
root=$(cd "$(dirname "$0")/../.." && pwd)
export CARGO_BUILD_JOBS=2 PYTHONDONTWRITEBYTECODE=1
exec python3 - "$root" <<'PY'
import gzip
import hashlib
import io
import json
import os
from pathlib import Path
import shutil
import subprocess
import sys
import tarfile
import tempfile
import tomllib

root = Path(sys.argv[1])
def git(*args):
    return subprocess.check_output(['git', '-C', str(root), *args])

commit = git('rev-parse', 'HEAD').decode().strip()
epoch = int(git('show', '-s', '--format=%ct', commit))
os.environ['SOURCE_DATE_EPOCH'] = str(epoch)
# Resolve the archived lockfile/manifests, never uncommitted local changes.
archive = git('archive', '--format=tar', commit)
with tempfile.TemporaryDirectory(prefix='atenea-audit-') as temp:
    tree = Path(temp)
    with tarfile.open(fileobj=io.BytesIO(archive)) as src:
        # git archive contains tracked paths; reject links before extraction.
        for member in src.getmembers():
            path = Path(member.name)
            if path.is_absolute() or '..' in path.parts or not (member.isfile() or member.isdir()):
                raise SystemExit('El archivo contiene una ruta o tipo no admitido')
        src.extractall(tree, filter='data')
    channel = tomllib.loads((tree / 'rust-toolchain.toml').read_text())['toolchain']['channel']
    # Resolve installed binaries without letting rustup proxies install a toolchain.
    def installed(tool):
        if shutil.which('rustup'):
            return subprocess.check_output(
                ['rustup', 'which', '--toolchain', channel, tool]).decode().strip()
        binary = shutil.which(tool)
        if binary is None:
            raise SystemExit('Falta herramienta local: ' + tool)
        return binary
    cargo = installed('cargo')
    os.environ['RUSTC'] = installed('rustc')
    metadata = json.loads(subprocess.check_output([
        cargo, 'metadata', '--offline', '--locked', '--format-version', '1'
    ], cwd=tree))
packages = metadata['packages']
identities = {p['id']: {'name': p['name'], 'version': p['version'],
                       'source': p['source'] or 'workspace'} for p in packages}
nodes = {n['id']: n for n in metadata['resolve']['nodes']}
condensed = []
for p in sorted(packages, key=lambda p: (p['name'], p['version'], p['source'] or '')):
    item = dict(identities[p['id']], license=p['license'])
    item['dependencies'] = sorted(
        [identities[d] for d in nodes[p['id']]['dependencies']],
        key=lambda d: (d['name'], d['version'], d['source']))
    condensed.append(item)
def json_bytes(value):
    return (json.dumps(value, ensure_ascii=False, indent=2, sort_keys=True) + '\n').encode()

deps = json_bytes({'commit': commit, 'packages': condensed,
                   'scope': 'cargo metadata; incluye desarrollo y todas las plataformas; excluye pnpm, SO y Wintun'})
provenance = json_bytes({'commit': commit, 'source_date_epoch': epoch,
                         'format': 'atenea-audit-snapshot/1'})
prefix = 'atenea-one-' + commit + '/'
raw = io.BytesIO()
with tarfile.open(fileobj=io.BytesIO(archive)) as src, tarfile.open(fileobj=raw, mode='w', format=tarfile.USTAR_FORMAT) as dst:
    members = {m.name: m for m in src.getmembers()}
    extra = {'AUDIT-SNAPSHOT.json': provenance, 'AUDIT-DEPENDENCIES.json': deps}
    if set(extra) & set(members):
        raise SystemExit('Colisión de nombres de procedencia')
    for name in sorted(set(members) | set(extra)):
        old = members.get(name)
        entry = tarfile.TarInfo(prefix + name)
        entry.uid = entry.gid = 0
        entry.uname = entry.gname = ''
        entry.mtime = epoch
        if old is not None and old.isdir():
            entry.type, entry.mode = tarfile.DIRTYPE, 0o755
            dst.addfile(entry)
        else:
            content = extra[name] if name in extra else src.extractfile(old).read()
            entry.mode = 0o755 if old is not None and old.mode & 0o111 else 0o644
            entry.size = len(content)
            dst.addfile(entry, io.BytesIO(content))
compressed = io.BytesIO()
with gzip.GzipFile(fileobj=compressed, mode='wb', filename='', mtime=epoch, compresslevel=9) as stream:
    stream.write(raw.getvalue())
files = {'source.tar.gz': compressed.getvalue(), 'COMMIT': (commit + '\n').encode(),
         'SOURCE_DATE_EPOCH': (str(epoch) + '\n').encode(), 'dependencies.json': deps}
files['SHA256SUMS'] = ''.join(hashlib.sha256(data).hexdigest() + '  ' + name + '\n'
                            for name, data in sorted(files.items())).encode()
out = root / 'dist' / 'audit'
out.mkdir(parents=True, exist_ok=True)
for name, data in files.items():
    (out / name).write_bytes(data)
print('Commit: ' + commit)
print('SHA256 source.tar.gz: ' + hashlib.sha256(files['source.tar.gz']).hexdigest())
print('Salida: dist/audit/ (solo HEAD; cambios sin commit excluidos)')
PY
