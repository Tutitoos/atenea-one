# Copyright 2026 Tutitoos
# SPDX-License-Identifier: Apache-2.0
"""El manifiesto firmado cubre los paquetes de la aplicación de escritorio (ADR-019, empaquetado): cada artefacto lleva el
SBOM Go de su sistema operativo, el .deb de nfpm (control.tar.gz) se acepta y su identidad se comprueba, y cualquier
alteración o fichero de más rompe la verificación. Sin Docker ni red; solo `openssl`."""
import gzip
import io
import json
import os
import subprocess
import sys
import tarfile
import tempfile
import unittest
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]
sys.path.insert(0, str(ROOT / 'scripts' / 'release'))
os.environ.setdefault('SOURCE_DATE_EPOCH', '1762000000')
import manifest  # noqa: E402
import sbom  # noqa: E402

COMMIT = '0123456789abcdef0123456789abcdef01234567'
VERSION = '0.1.12'


def deb(version, commit, control_name='control.tar.gz'):
    """Un .deb mínimo con el mismo formato ar que produce nfpm."""
    control = f'Package: atenea-desktop\nVersion: {version}\nX-Atenea-Commit: {commit}\n'
    buffer = io.BytesIO()
    with tarfile.open(fileobj=buffer, mode='w') as tar:
        data = control.encode()
        info = tarfile.TarInfo('./control')
        info.size = len(data)
        tar.addfile(info, io.BytesIO(data))
    members = [('debian-binary', b'2.0\n'), (control_name, gzip.compress(buffer.getvalue())), ('data.tar.gz', gzip.compress(b''))]
    out = b'!<arch>\n'
    for name, content in members:
        out += f'{name + "/":<16}{0:<12}{0:<6}{0:<6}{100644:<8}{len(content):<10}`\n'.encode() + content
        if len(content) % 2:
            out += b'\n'
    return out


class DesktopManifest(unittest.TestCase):
    def setUp(self):
        self.tmp = tempfile.TemporaryDirectory()
        self.addCleanup(self.tmp.cleanup)
        tmp = Path(self.tmp.name)
        self.dist = tmp / 'dist'
        self.dist.mkdir()
        self.key = tmp / 'k.pem'
        self.pub = tmp / 'k.pub'
        subprocess.run(['openssl', 'genpkey', '-algorithm', 'ED25519', '-out', self.key], check=True, capture_output=True)
        subprocess.run(['openssl', 'pkey', '-in', self.key, '-pubout', '-out', self.pub], check=True, capture_output=True)
        identity = json.dumps({'commit': COMMIT, 'version': VERSION})
        (self.dist / 'BUILD-INFO.json').write_text(json.dumps({'commit': COMMIT, 'version': VERSION, 'toolchain': 'rustc 0 (synthetic)'}))
        (self.dist / 'images.json').write_text('[]')
        listing = tmp / 'golist.txt'
        listing.write_text('github.com/wailsapp/wails/v3\tv3.0.0\n')
        gosum = tmp / 'go.sum'
        gosum.write_text('github.com/wailsapp/wails/v3 v3.0.0 h1:abc=\ngithub.com/wailsapp/wails/v3 v3.0.0/go.mod h1:def=\n')
        for system, target in (('Linux', 'linux/amd64'), ('macOS', 'darwin/arm64'), ('Windows', 'windows/amd64')):
            sbom.emit('go', str(listing), str(self.dist / f'go-desktop-{system}.cdx.json'), VERSION, COMMIT, target=target, gosum=str(gosum))
            (self.dist / f'desktop-{system}.build-info.json').write_text(identity)
        (self.dist / f'atenea-desktop_{VERSION}_amd64.deb').write_bytes(deb(VERSION, COMMIT))
        (self.dist / f'atenea-desktop-{VERSION}.x86_64.rpm').write_bytes(b'rpm')
        (self.dist / f'atenea-desktop-{VERSION}-macOS-arm64.dmg').write_bytes(b'dmg')
        (self.dist / f'atenea-desktop-{VERSION}-Windows-amd64.exe').write_bytes(b'exe')

    def create(self):
        manifest.create(self.dist, str(self.key), VERSION, COMMIT)
        return json.loads((self.dist / 'release-manifest.json').read_text())

    def test_each_package_is_tied_to_the_go_sbom_of_its_operating_system(self):
        by_name = {a['name']: a.get('sboms') for a in self.create()['artifacts']}
        self.assertEqual(by_name[f'atenea-desktop_{VERSION}_amd64.deb'], ['go-desktop-Linux.cdx.json'])
        self.assertEqual(by_name[f'atenea-desktop-{VERSION}.x86_64.rpm'], ['go-desktop-Linux.cdx.json'])
        self.assertEqual(by_name[f'atenea-desktop-{VERSION}-macOS-arm64.dmg'], ['go-desktop-macOS.cdx.json'])
        self.assertEqual(by_name[f'atenea-desktop-{VERSION}-Windows-amd64.exe'], ['go-desktop-Windows.cdx.json'])

    def test_verification_passes_for_an_untouched_release(self):
        self.create()
        manifest.check(self.dist, json.loads((self.dist / 'release-manifest.json').read_text()), COMMIT, VERSION)

    def test_a_deb_built_for_another_commit_is_rejected(self):
        (self.dist / f'atenea-desktop_{VERSION}_amd64.deb').write_bytes(deb(VERSION, 'f' * 40))
        with self.assertRaisesRegex(AssertionError, 'Identidad no coincide'):
            self.create()

    def test_a_deb_without_a_control_member_is_rejected(self):
        (self.dist / f'atenea-desktop_{VERSION}_amd64.deb').write_bytes(deb(VERSION, COMMIT, control_name='otro.tar.gz'))
        with self.assertRaises(AssertionError):
            self.create()

    def test_a_modified_package_or_an_extra_file_breaks_verification(self):
        manifest_data = self.create()
        (self.dist / f'atenea-desktop-{VERSION}-macOS-arm64.dmg').write_bytes(b'alterado')
        with self.assertRaises(AssertionError):
            manifest.check(self.dist, manifest_data, COMMIT, VERSION)
        (self.dist / f'atenea-desktop-{VERSION}-macOS-arm64.dmg').write_bytes(b'dmg')
        manifest.check(self.dist, manifest_data, COMMIT, VERSION)
        (self.dist / 'intruso.exe').write_bytes(b'x')
        with self.assertRaises(AssertionError):
            manifest.check(self.dist, manifest_data, COMMIT, VERSION)


class GoSbom(unittest.TestCase):
    def test_a_module_missing_from_go_sum_is_rejected_and_an_empty_listing_too(self):
        with tempfile.TemporaryDirectory() as tmp:
            listing = Path(tmp) / 'l.txt'
            gosum = Path(tmp) / 'go.sum'
            gosum.write_text('github.com/a/b v1.0.0 h1:x=\n')
            listing.write_text('github.com/c/d\tv1.0.0\n')
            with self.assertRaises(AssertionError):
                sbom.golang(str(listing), str(gosum))
            listing.write_text('\n')
            with self.assertRaises(AssertionError):
                sbom.golang(str(listing), str(gosum))


if __name__ == '__main__':
    unittest.main()
