# Copyright 2026 Tutitoos
# SPDX-License-Identifier: Apache-2.0
"""El manifiesto firmado cubre los paquetes de la aplicación de escritorio (ADR-019, empaquetado): cada artefacto lleva el
SBOM Go de su sistema operativo, el .deb de nfpm (control.tar.gz) se acepta y su identidad se comprueba, y cualquier
alteración o fichero de más rompe la verificación. `verify.sh --published` verifica con ese mismo manifiesto lo que se
descarga del release publicado (solo los instaladores). Sin Docker ni red; solo `openssl`."""
import gzip
import io
import json
import os
import shutil
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
VERIFY = ROOT / 'scripts' / 'release' / 'verify.sh'


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
        (self.dist / f'atenea-desktop-{VERSION}-linux-amd64.deb').write_bytes(deb(VERSION, COMMIT))
        (self.dist / f'atenea-desktop-{VERSION}-linux-amd64.rpm').write_bytes(b'rpm')
        (self.dist / f'atenea-desktop-{VERSION}-macos-arm64.dmg').write_bytes(b'dmg')
        (self.dist / f'atenea-desktop-{VERSION}-windows-amd64.exe').write_bytes(b'exe')

    def create(self):
        manifest.create(self.dist, str(self.key), VERSION, COMMIT)
        return json.loads((self.dist / 'release-manifest.json').read_text())

    def published(self):
        """Lo que lleva el release de GitHub: solo los instaladores, SHA256SUMS, el manifiesto y su firma."""
        self.create()
        out = Path(self.tmp.name) / 'published'
        out.mkdir()
        for path in self.dist.iterdir():
            if path.suffix in ('.deb', '.rpm', '.dmg', '.exe') or path.name in ('SHA256SUMS', 'release-manifest.json', 'release-manifest.json.sig'):
                shutil.copy(path, out / path.name)
        return out

    def verify(self, directory, *flags, pub=None, commit=COMMIT):
        return subprocess.run(['bash', str(VERIFY), *flags, str(directory), str(pub or self.pub), commit, VERSION], capture_output=True, text=True)

    def test_each_package_is_tied_to_the_go_sbom_of_its_operating_system(self):
        by_name = {a['name']: a.get('sboms') for a in self.create()['artifacts']}
        self.assertEqual(by_name[f'atenea-desktop-{VERSION}-linux-amd64.deb'], ['go-desktop-Linux.cdx.json'])
        self.assertEqual(by_name[f'atenea-desktop-{VERSION}-linux-amd64.rpm'], ['go-desktop-Linux.cdx.json'])
        self.assertEqual(by_name[f'atenea-desktop-{VERSION}-macos-arm64.dmg'], ['go-desktop-macOS.cdx.json'])
        self.assertEqual(by_name[f'atenea-desktop-{VERSION}-windows-amd64.exe'], ['go-desktop-Windows.cdx.json'])

    def test_the_single_macos_installer_is_tied_to_both_the_rust_and_the_go_sbom(self):
        # El inventario Rust de macOS se simula con el de Go: solo importa que exista, sea válido y lleve la identidad.
        shutil.copy(self.dist / 'go-desktop-macOS.cdx.json', self.dist / 'rust-macOS.cdx.json')
        (self.dist / f'atenea-one-{VERSION}-macos-arm64.pkg').write_bytes(b'pkg')
        by_name = {a['name']: a.get('sboms') for a in self.create()['artifacts']}
        self.assertEqual(by_name[f'atenea-one-{VERSION}-macos-arm64.pkg'], ['rust-macOS.cdx.json', 'go-desktop-macOS.cdx.json'])

    def test_verification_passes_for_an_untouched_release(self):
        self.create()
        manifest.check(self.dist, json.loads((self.dist / 'release-manifest.json').read_text()), COMMIT, VERSION)

    def test_a_deb_built_for_another_commit_is_rejected(self):
        (self.dist / f'atenea-desktop-{VERSION}-linux-amd64.deb').write_bytes(deb(VERSION, 'f' * 40))
        with self.assertRaisesRegex(AssertionError, 'Identidad no coincide'):
            self.create()

    def test_a_deb_without_a_control_member_is_rejected(self):
        (self.dist / f'atenea-desktop-{VERSION}-linux-amd64.deb').write_bytes(deb(VERSION, COMMIT, control_name='otro.tar.gz'))
        with self.assertRaises(AssertionError):
            self.create()

    def test_a_modified_package_or_an_extra_file_breaks_verification(self):
        manifest_data = self.create()
        (self.dist / f'atenea-desktop-{VERSION}-macos-arm64.dmg').write_bytes(b'alterado')
        with self.assertRaises(AssertionError):
            manifest.check(self.dist, manifest_data, COMMIT, VERSION)
        (self.dist / f'atenea-desktop-{VERSION}-macos-arm64.dmg').write_bytes(b'dmg')
        manifest.check(self.dist, manifest_data, COMMIT, VERSION)
        (self.dist / 'intruso.exe').write_bytes(b'x')
        with self.assertRaises(AssertionError):
            manifest.check(self.dist, manifest_data, COMMIT, VERSION)

    def test_a_published_subset_passes_and_what_was_not_downloaded_is_ignored(self):
        out = self.published()
        result = self.verify(out, '--published')
        self.assertEqual(result.returncode, 0, result.stdout + result.stderr)
        self.assertIn('PASS', result.stdout)
        # Una sola descarga también vale, sin SHA256SUMS ni los demás instaladores.
        for path in list(out.iterdir()):
            if path.suffix != '.dmg' and path.name not in ('release-manifest.json', 'release-manifest.json.sig'):
                path.unlink()
        result = self.verify(out, '--published')
        self.assertEqual(result.returncode, 0, result.stdout + result.stderr)

    def test_the_full_verification_still_rejects_the_published_subset(self):
        self.assertNotEqual(self.verify(self.published()).returncode, 0)

    def test_a_tampered_installer_fails_in_published_mode(self):
        out = self.published()
        (out / f'atenea-desktop-{VERSION}-macos-arm64.dmg').write_bytes(b'alterado')
        result = self.verify(out, '--published')
        self.assertNotEqual(result.returncode, 0)
        self.assertIn(f'atenea-desktop-{VERSION}-macos-arm64.dmg', result.stderr)

    def test_a_foreign_file_or_a_tampered_checksum_list_fails_in_published_mode(self):
        out = self.published()
        (out / 'intruso.exe').write_bytes(b'x')
        result = self.verify(out, '--published')
        self.assertNotEqual(result.returncode, 0)
        self.assertIn('Ficheros no inventariados: intruso.exe', result.stderr)
        (out / 'intruso.exe').unlink()
        self.assertEqual(self.verify(out, '--published').returncode, 0)
        (out / 'SHA256SUMS').write_text('0' * 64 + '  atenea-desktop.exe\n')
        result = self.verify(out, '--published')
        self.assertNotEqual(result.returncode, 0)
        self.assertIn('SHA256SUMS alterado', result.stderr)

    def test_a_directory_without_any_manifest_artifact_fails_in_published_mode(self):
        out = self.published()
        for path in list(out.iterdir()):
            if path.name not in ('release-manifest.json', 'release-manifest.json.sig'):
                path.unlink()
        result = self.verify(out, '--published')
        self.assertNotEqual(result.returncode, 0)
        self.assertIn('artefacto del manifiesto', result.stderr)

    def test_published_mode_still_demands_the_signing_key_and_the_expected_commit(self):
        out = self.published()
        key = Path(self.tmp.name) / 'otra.pem'
        other = Path(self.tmp.name) / 'otra.pub'
        subprocess.run(['openssl', 'genpkey', '-algorithm', 'ED25519', '-out', key], check=True, capture_output=True)
        subprocess.run(['openssl', 'pkey', '-in', key, '-pubout', '-out', other], check=True, capture_output=True)
        self.assertNotEqual(self.verify(out, '--published', pub=other).returncode, 0)
        self.assertNotEqual(self.verify(out, '--published', commit='f' * 40).returncode, 0)


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
