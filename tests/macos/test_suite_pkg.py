# Copyright 2026 Tutitoos
# SPDX-License-Identifier: Apache-2.0
"""El instalador único de macOS (ADR-022) debe llevar el cliente SIN alterarlo y la aplicación como propiedad de root.

Dos fallos serían silenciosos y graves: perder el `postinstall` del cliente (los servicios no se reiniciarían al
actualizar) o dejar `Atenea One.app` en manos del usuario (un proceso suyo podría sustituirla y el sistema la ejecutaría
después). Se construye con binarios y una aplicación sintéticos; no instala nada y no usa sudo.
"""
import filecmp
import os
import platform
import plistlib
import shutil
import subprocess
import tempfile
import unittest
from pathlib import Path

REPO = Path(__file__).resolve().parents[2]
VERSION = '9.8.7'


def run(*args, env=None):
    return subprocess.run(args, check=True, capture_output=True, text=True, env=env)


@unittest.skipUnless(platform.system() == 'Darwin', 'necesita pkgbuild, productbuild y pkgutil de macOS')
class SuitePackage(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        cls.tmp = Path(tempfile.mkdtemp())
        bins = cls.tmp / 'bins'
        bins.mkdir()
        for name in ('atenea', 'atenea-agent'):
            (bins / name).write_text('#!/bin/sh\nexit 0\n')
            (bins / name).chmod(0o755)
        env = {**os.environ, 'ATENEA_BIN_DIR': str(bins), 'ATENEA_VERSION': VERSION}
        out = run(str(REPO / 'packaging/macos/build-pkg.sh'), env=env).stdout.strip().splitlines()[-1]
        cls.client = Path(out)
        cls.app = cls.make_app(cls.tmp / 'app', VERSION)

    @classmethod
    def tearDownClass(cls):
        shutil.rmtree(cls.tmp, ignore_errors=True)
        cls.client.unlink(missing_ok=True)

    @staticmethod
    def make_app(root, version):
        app = root / 'Atenea One.app'
        (app / 'Contents/MacOS').mkdir(parents=True)
        binary = app / 'Contents/MacOS/atenea-desktop'
        binary.write_text('#!/bin/sh\nexit 0\n')
        binary.chmod(0o755)
        with (app / 'Contents/Info.plist').open('wb') as f:
            plistlib.dump({'CFBundleIdentifier': 'one.atenea.desktop', 'CFBundleExecutable': 'atenea-desktop',
                           'CFBundleShortVersionString': version, 'CFBundleVersion': version}, f)
        run('codesign', '--force', '--deep', '--sign', '-', str(app))
        return app

    def build(self, version=VERSION, app=None):
        return Path(run(str(REPO / 'packaging/macos/build-suite-pkg.sh'), str(self.client), str(app or self.app), version,
                        str(self.tmp / f'out-{version}')).stdout.strip())

    def test_the_client_component_keeps_its_postinstall_and_the_app_is_owned_by_root(self):
        pkg = self.build()
        self.assertRegex(pkg.name, rf'^atenea-one-{VERSION}-macos-(arm64|amd64)\.pkg$')
        expanded = self.tmp / 'expanded'
        run('pkgutil', '--expand-full', str(pkg), str(expanded))
        self.assertTrue(filecmp.cmp(expanded / 'client.pkg/Scripts/postinstall', REPO / 'packaging/macos/postinstall', shallow=False))
        self.assertTrue(os.access(expanded / 'client.pkg/Scripts/postinstall', os.X_OK))
        info = (expanded / 'desktop.pkg/PackageInfo').read_text()
        self.assertIn('install-location="/Applications"', info)
        self.assertIn('relocatable="false"', info)  # si no, el instalador «actualizaría» una copia de la carpeta del usuario
        for component, wanted in (('desktop.pkg', 'Atenea One.app'), ('client.pkg', 'Library/Atenea/bin/atenea')):
            bom = run('lsbom', str(expanded / component / 'Bom')).stdout.splitlines()
            owned = [line.split('\t') for line in bom if line.startswith(f'./{wanted}')]
            self.assertTrue(owned, wanted)
            # Propietario y grupo 0/0 (root:wheel); `._*` son metadatos AppleDouble que añade pkgbuild en equipos con xattrs.
            self.assertTrue(all(fields[2].startswith('0/0') for fields in owned), owned)

    def test_the_installed_app_is_the_one_that_was_signed(self):
        expanded = self.tmp / 'expanded-sig'
        run('pkgutil', '--expand-full', str(self.build()), str(expanded))
        run('codesign', '--verify', '--deep', '--strict', str(expanded / 'desktop.pkg/Payload/Atenea One.app'))

    def test_an_app_of_another_version_is_rejected(self):
        old = self.make_app(self.tmp / 'old', '1.0.0')
        with self.assertRaises(subprocess.CalledProcessError) as raised:
            self.build(app=old)
        self.assertIn('1.0.0', raised.exception.stderr)

    def test_a_client_of_another_version_is_rejected(self):
        with self.assertRaises(subprocess.CalledProcessError) as raised:
            run(str(REPO / 'packaging/macos/build-suite-pkg.sh'), str(self.client), str(self.make_app(self.tmp / 'nueva', '9.8.8')),
                '9.8.8', str(self.tmp / 'out-9.8.8'))
        self.assertIn(VERSION, raised.exception.stderr)

    def test_a_dirty_version_string_is_rejected(self):
        for bad in ('x1', '1.0;rm', ''):
            with self.assertRaises(subprocess.CalledProcessError):
                self.build(version=bad)


if __name__ == '__main__':
    unittest.main()
