# Copyright 2026 Tutitoos
# SPDX-License-Identifier: Apache-2.0
"""El .pkg quita `ProcessType = Background` de los servicios de red ya instalados.

Con ese valor macOS frena el agente cuando el equipo va cargado: dejaba de contestar a la app y a sus plazos de red.
Se prueba, sin privilegios y sobre un plist temporal, la función real del instalador (`normal_priority`)."""
from pathlib import Path
import plistlib
import subprocess
import tempfile
import unittest

HELPER = Path(__file__).resolve().parents[2] / "packaging/macos/atenea-network.sh"


def normal_priority(path):
    return subprocess.run(["/bin/sh", "-c", f'. "{HELPER}" && normal_priority "{path}"'], capture_output=True)


class NormalPriority(unittest.TestCase):
    def setUp(self):
        self.tmp = tempfile.TemporaryDirectory()
        self.addCleanup(self.tmp.cleanup)
        self.plist = Path(self.tmp.name) / "one.atenea.network.12345678.plist"

    def write(self, **extra):
        self.plist.write_bytes(plistlib.dumps({"Label": "one.atenea.network.12345678", "RunAtLoad": True, **extra}))

    def read(self):
        return plistlib.loads(self.plist.read_bytes())

    def test_background_is_removed_and_the_rest_is_kept(self):
        self.write(ProcessType="Background", ThrottleInterval=10)
        self.assertEqual(normal_priority(self.plist).returncode, 0)
        self.assertEqual(self.read(), {"Label": "one.atenea.network.12345678", "RunAtLoad": True, "ThrottleInterval": 10})

    def test_a_value_chosen_by_the_administrator_is_not_touched(self):
        for value in ("Interactive", "Adaptive", "Standard"):
            self.write(ProcessType=value)
            self.assertEqual(normal_priority(self.plist).returncode, 0)
            self.assertEqual(self.read()["ProcessType"], value)

    def test_a_plist_without_the_key_is_left_alone(self):
        self.write()
        before = self.plist.read_bytes()
        self.assertEqual(normal_priority(self.plist).returncode, 0)
        self.assertEqual(self.plist.read_bytes(), before)

    def test_running_it_twice_changes_nothing_the_second_time(self):
        self.write(ProcessType="Background")
        normal_priority(self.plist)
        once = self.plist.read_bytes()
        self.assertEqual(normal_priority(self.plist).returncode, 0)
        self.assertEqual(self.plist.read_bytes(), once)


if __name__ == "__main__":
    unittest.main()
