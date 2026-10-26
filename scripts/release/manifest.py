#!/usr/bin/env python3
# Copyright 2026 Tutitoos
# SPDX-License-Identifier: Apache-2.0
"""Ensamblado y verificación estricta del release; la confianza llega por clave externa."""
import argparse
import datetime
import hashlib
import io
import json
import os
import re
import subprocess
import tarfile
import zipfile
from pathlib import Path
from sbom import validate

if not __debug__:
    raise RuntimeError('La verificación requiere Python sin -O/PYTHONOPTIMIZE')


def ed25519(key, public=False):
    args = ['openssl', 'pkey', '-in', key, '-pubout', '-outform', 'DER']
    if public:
        args.append('-pubin')
    der = subprocess.check_output(args)
    assert len(der) == 44 and der.startswith(bytes.fromhex('302a300506032b6570032100')), 'Se exige clave Ed25519'


def digest(path):
    with path.open('rb') as stream:
        return hashlib.file_digest(stream, 'sha256').hexdigest()


def identity(info, version, commit):
    assert info['version'] == version and info['commit'] == commit, 'Identidad no coincide'


def check(dist, manifest, commit, version):
    assert re.fullmatch(r'[0-9a-f]{40}', manifest['commit'])
    assert re.fullmatch(r'(0|[1-9]\d*)\.(0|[1-9]\d*)\.(0|[1-9]\d*)(?:-[0-9A-Za-z.-]+)?(?:\+[0-9A-Za-z.-]+)?', manifest['version'])
    identity(manifest, version, commit)
    assert manifest['toolchain'] and manifest['date']
    listed = set()
    for entry in manifest['artifacts'] + manifest['sboms']:
        name = entry['name']
        assert name not in listed and Path(name).name == name and name not in ('.', '..')
        listed.add(name)
        path = dist / name
        assert path.is_file() and not path.is_symlink()
        assert path.stat().st_size == entry['size'] and digest(path) == entry['sha256'], name
        if entry in manifest['sboms']:
            bom = json.loads(path.read_text())
            validate(bom)
            assert bom['metadata']['component']['version'] == version
            assert {'name': 'atenea:commit', 'value': commit} in bom['metadata']['properties']
    assert 'BUILD-INFO.json' in listed
    identity(json.loads((dist / 'BUILD-INFO.json').read_text()), version, commit)
    for entry in manifest['artifacts']:
        if entry['name'].endswith('.build-info.json'):
            identity(json.loads((dist / entry['name']).read_text()), version, commit)
        for ref in entry.get('sboms', []):
            assert ref in {e['name'] for e in manifest['sboms']}
    assert json.loads((dist / 'images.json').read_text()) == manifest['images']
    for entry in manifest['artifacts']:
        path = dist / entry['name']
        if path.suffix == '.zip':
            with zipfile.ZipFile(path) as archive:
                identity(json.loads(archive.read('BUILD-INFO.json').decode('utf-8-sig')), version, commit)
        if path.suffix == '.deb':
            with path.open('rb') as archive:
                assert archive.read(8) == b'!<arch>\n'
                control = None
                while header := archive.read(60):
                    assert len(header) == 60 and header[58:] == b'`\n'
                    size = int(header[48:58])
                    content = archive.read(size)
                    assert len(content) == size
                    if header[:16].decode().strip().rstrip('/') == 'control.tar.xz':
                        control = content
                    if size % 2:
                        archive.read(1)
                assert control is not None
            with tarfile.open(fileobj=io.BytesIO(control)) as archive:
                fields = dict(line.split(': ', 1) for line in archive.extractfile('./control').read().decode().splitlines() if ': ' in line and not line.startswith(' '))
                identity({'version': fields['Version'], 'commit': fields['X-Atenea-Commit']}, version, commit)
    expected = ''.join(f"{e['sha256']}  {e['name']}\n" for e in sorted(manifest['artifacts'] + manifest['sboms'], key=lambda e: e['name']))
    assert (dist / 'SHA256SUMS').read_text() == expected, 'SHA256SUMS alterado'
    assert {p.name for p in dist.iterdir()} == listed | {'SHA256SUMS', 'release-manifest.json', 'release-manifest.json.sig'}, 'Ficheros no inventariados'
    for image in manifest['images']:
        identity(image, version, commit)
        assert image['id'].startswith('sha256:')
        assert image['registry_digest'] is None or re.fullmatch(r'.+@sha256:[0-9a-f]{64}', image['registry_digest'])
        assert image['archive'] in listed and image['sbom'] in listed and image['manifest'] in listed
        assert json.loads((dist / image['manifest']).read_text())['config']['digest'] == image['config_digest']
        assert 'sha256:' + digest(dist / image['manifest']) == image['manifest_digest']


def create(dist, key, version, commit):
    assert re.fullmatch(r'[0-9a-f]{40}', commit)
    ed25519(key)
    date = datetime.datetime.fromtimestamp(int(os.environ['SOURCE_DATE_EPOCH']), datetime.timezone.utc).isoformat().replace('+00:00', 'Z')
    entries = []
    for path in sorted(dist.iterdir()):
        if path.name not in ('release-manifest.json', 'release-manifest.json.sig', 'SHA256SUMS'):
            assert path.is_file() and not path.is_symlink()
            entries.append({'name': path.name, 'size': path.stat().st_size, 'sha256': digest(path)})
    manifest = {'version': version, 'commit': commit, 'date': date,
                'toolchain': json.loads((dist / 'BUILD-INFO.json').read_text())['toolchain'],
                'images': json.loads((dist / 'images.json').read_text()),
                'artifacts': [e for e in entries if not e['name'].endswith('.cdx.json')],
                'sboms': [e for e in entries if e['name'].endswith('.cdx.json')]}
    for entry in manifest['artifacts']:
        name = entry['name']
        if name.endswith(('.pkg', '-macOS')):
            entry['sboms'] = ['rust-macOS.cdx.json']
        elif name.endswith(('.zip', '.exe')):
            entry['sboms'] = ['rust-Windows.cdx.json']
        elif name.endswith(('-linux', '.deb')):
            entry['sboms'] = ['rust.cdx.json']
        elif name.endswith('.docker.tar'):
            entry['sboms'] = [name.removesuffix('.docker.tar') + '.cdx.json', 'rust.cdx.json']
            if name.startswith('control-plane'):
                entry['sboms'].append('panel.cdx.json')
    (dist / 'SHA256SUMS').write_text(''.join(f"{e['sha256']}  {e['name']}\n" for e in entries))
    target = dist / 'release-manifest.json'
    target.write_text(json.dumps(manifest, sort_keys=True, indent=2) + '\n')
    subprocess.run(['openssl', 'pkeyutl', '-sign', '-rawin', '-inkey', key, '-in', str(target), '-out', str(target) + '.sig'], check=True)
    check(dist, manifest, commit, version)


if __name__ == '__main__':
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('mode', choices=['create', 'verify'])
    parser.add_argument('dist', type=Path)
    parser.add_argument('key')
    parser.add_argument('commit')
    parser.add_argument('version')
    args = parser.parse_args()
    if args.mode == 'create':
        create(args.dist, args.key, args.version, args.commit)
    else:
        target = args.dist / 'release-manifest.json'
        ed25519(args.key, public=True)
        subprocess.run(['openssl', 'pkeyutl', '-verify', '-pubin', '-rawin', '-inkey', args.key, '-in', str(target), '-sigfile', str(target) + '.sig'], check=True)
        check(args.dist, json.loads(target.read_text()), args.commit, args.version)
        print('PASS firma Ed25519, identidad, artefactos, SBOM y SHA256SUMS')
