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


class RepairRunDir(unittest.TestCase):
    """`repair_run_dir`: sube de 0700 a 0755 solo en el caso de la 0.1.13 (redes por instancia, sin instalación heredada)."""

    def setUp(self):
        self.tmp = tempfile.TemporaryDirectory()
        self.addCleanup(self.tmp.cleanup)
        root = Path(self.tmp.name)
        self.dir = root / "atenea"
        self.legacy = root / "agent.toml"
        self.plists = root / "LaunchDaemons"
        self.plists.mkdir()

    def network(self):
        (self.plists / "one.atenea.network.0a1b2c3d.plist").write_text("x")

    def repair(self, owner=None):
        owner = os.getuid() if owner is None else owner
        subprocess.run(["/bin/sh", "-c", f'. "{HELPER}" && repair_run_dir "{self.dir}" "{self.legacy}" "{self.plists}" "{owner}"'], check=True)

    def make(self, wanted):
        self.dir.mkdir()
        self.dir.chmod(wanted)

    def test_the_0_1_13_state_is_repaired(self):
        self.make(0o700)
        self.network()
        self.repair()
        self.assertEqual(mode(self.dir), 0o755)

    def test_nothing_else_is_touched(self):
        # 0750 es `status_gid`; 0711 y 0755 ya valen; 0770 no lo deja ningún lanzador. Sin red por instancia o con instalación heredada no se repara.
        for wanted in (0o750, 0o711, 0o755, 0o770):
            with self.subTest(modo=oct(wanted)):
                self.make(wanted)
                self.network()
                self.repair()
                self.assertEqual(mode(self.dir), wanted)
                self.dir.rmdir()
        self.make(0o700)
        for stale in ("sin_red", "con_heredada"):
            with self.subTest(caso=stale):
                for plist in self.plists.iterdir():
                    plist.unlink()
                if stale == "con_heredada":
                    self.network()
                    self.legacy.write_text("x")
                self.repair()
                self.assertEqual(mode(self.dir), 0o700)

    def test_a_directory_of_another_owner_or_a_symlink_is_not_repaired(self):
        self.make(0o700)
        self.network()
        self.repair(owner=os.getuid() + 1)
        self.assertEqual(mode(self.dir), 0o700)
        link = Path(self.tmp.name) / "enlace"
        link.symlink_to(self.dir)
        self.dir = link
        self.repair()
        self.assertEqual(mode(self.dir.resolve()), 0o700)

    def test_a_missing_directory_is_left_missing(self):
        self.network()
        self.repair()
        self.assertFalse(self.dir.exists())


class TrustedDir(unittest.TestCase):
    """`trusted_dir`: el desinstalador solo borra sockets dentro de directorios reales del propietario y cerrados a otros."""

    def setUp(self):
        self.tmp = tempfile.TemporaryDirectory()
        self.addCleanup(self.tmp.cleanup)
        self.dir = Path(self.tmp.name) / "atenea"

    def trusted(self, path, uid=None):
        uid = os.getuid() if uid is None else uid
        return subprocess.run(["/bin/sh", "-c", f'. "{HELPER}" && trusted_dir "{path}" {uid}']).returncode == 0

    def test_the_modes_left_by_the_launchers_are_trusted(self):
        # El padre del directorio temporal es escribible por más gente, como /private/var/run: no debe importar.
        for wanted in (0o700, 0o750, 0o711, 0o755):
            with self.subTest(mode=oct(wanted)):
                self.dir.mkdir()
                self.dir.chmod(wanted)
                self.assertTrue(self.trusted(self.dir))
                self.dir.rmdir()

    def test_a_directory_writable_by_group_or_others_is_rejected(self):
        for wanted in (0o770, 0o757, 0o777):
            with self.subTest(mode=oct(wanted)):
                self.dir.mkdir()
                self.dir.chmod(wanted)
                self.assertFalse(self.trusted(self.dir))
                self.dir.rmdir()

    def test_a_symlink_a_file_another_owner_and_a_missing_path_are_rejected(self):
        self.assertFalse(self.trusted(self.dir))
        self.dir.mkdir(mode=0o755)
        self.assertFalse(self.trusted(self.dir, uid=os.getuid() + 1))
        link = Path(self.tmp.name) / "enlace"
        link.symlink_to(self.dir)
        self.assertFalse(self.trusted(link))
        regular = Path(self.tmp.name) / "fichero"
        regular.write_text("")
        regular.chmod(0o644)
        self.assertFalse(self.trusted(regular))


if __name__ == "__main__":
    unittest.main()
