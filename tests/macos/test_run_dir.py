# Copyright 2026 Tutitoos
# SPDX-License-Identifier: Apache-2.0
"""El .pkg no debe cambiar los permisos de /var/run/atenea al actualizar.

La v0.1.13 hacía `install -d -m 0700 /var/run/atenea` incondicionalmente: con las redes unidas ya en marcha, los usuarios
sin privilegios perdían el acceso a sus sockets de estado y `atenea network list` y la aplicación enseñaban todas las redes
como «detenida» hasta reiniciar. Aquí se prueba, sin privilegios y sobre un directorio temporal, la función real del
instalador (`ensure_run_dir`, en atenea-network.sh)."""
import grp
import os
from pathlib import Path
import pwd
import stat
import subprocess
import tempfile
import unittest

HELPER = Path(__file__).resolve().parents[2] / "packaging/macos/atenea-network.sh"
USER = pwd.getpwuid(os.getuid()).pw_name
GROUP = grp.getgrgid(os.getgid()).gr_name


def ensure(path):
    """Ejecuta ensure_run_dir del instalador con el usuario y el grupo actuales (la producción usa root:wheel)."""
    subprocess.run(["/bin/sh", "-c", f'. "{HELPER}" && ensure_run_dir "{path}" "{USER}" "{GROUP}"'], check=True)


def mode(path):
    return stat.S_IMODE(os.stat(path).st_mode)


class RunDir(unittest.TestCase):
    def setUp(self):
        self.tmp = tempfile.TemporaryDirectory()
        self.addCleanup(self.tmp.cleanup)
        self.dir = Path(self.tmp.name) / "atenea"

    def test_a_missing_directory_is_created_traversable_by_everyone(self):
        ensure(self.dir)
        self.assertTrue(self.dir.is_dir())
        self.assertEqual(mode(self.dir), 0o755)
        self.assertEqual(self.dir.stat().st_uid, os.getuid())

    def test_an_existing_directory_keeps_its_permissions_whatever_they_are(self):
        # 0700 y 0750 son los que deja el lanzador heredado según `status_gid`; 0711 y 0755, los de una red.
        for wanted in (0o700, 0o750, 0o711, 0o755, 0o770):
            with self.subTest(modo=oct(wanted)):
                self.dir.mkdir(exist_ok=True)
                self.dir.chmod(wanted)
                ensure(self.dir)
                self.assertEqual(mode(self.dir), wanted)
                self.dir.rmdir()

    def test_the_contents_of_an_existing_directory_are_left_alone(self):
        self.dir.mkdir(mode=0o750)
        self.dir.chmod(0o750)
        (self.dir / "0a1b2c3d").mkdir()
        (self.dir / "0a1b2c3d").chmod(0o750)
        socket_stub = self.dir / "0a1b2c3d" / "status.sock"
        socket_stub.write_text("x")
        socket_stub.chmod(0o660)
        ensure(self.dir)
        self.assertEqual(mode(self.dir), 0o750)
        self.assertEqual(mode(self.dir / "0a1b2c3d"), 0o750)
        self.assertEqual(mode(socket_stub), 0o660)

    def test_a_symlink_to_a_directory_is_not_replaced_or_followed_to_chmod(self):
        # Los lanzadores rechazan un symlink con 78; el instalador no debe «arreglarlo» ni cambiar el destino.
        target = Path(self.tmp.name) / "otro"
        target.mkdir()
        target.chmod(0o700)
        self.dir.symlink_to(target)
        ensure(self.dir)
        self.assertTrue(self.dir.is_symlink())
        self.assertEqual(mode(target), 0o700)


if __name__ == "__main__":
    unittest.main()
