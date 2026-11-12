# Copyright 2026 Tutitoos
# SPDX-License-Identifier: Apache-2.0
"""`stop_service` (atenea-network.sh): el desinstalador no debe limpiar mientras el agente aún se está parando.

`launchctl bootout` es asíncrono: el servicio sigue cargado hasta que su proceso sale. Si el desinstalador ejecutaba el
`cleanup` fuera de línea justo después, competía con la limpieza del propio agente y a veces fallaba (la CI de macOS vio el
desinstalador salir con 1). Aquí se prueba, sin privilegios, la función real con un `launchctl` falso que mantiene el servicio
«cargado» durante un número fijo de consultas."""
import os
from pathlib import Path
import stat
import subprocess
import tempfile
import unittest

HELPER = Path(__file__).resolve().parents[2] / "packaging/macos/atenea-network.sh"
FAKE_LAUNCHCTL = """#!/bin/sh
# print: sale 0 mientras queden consultas pendientes en $STATE/loaded; bootout: registra la orden.
state=$(dirname "$0")
case "$1" in
bootout) echo "$2" >> "$state/bootout.log" ;;
print)
    echo "$2" >> "$state/print.log"
    left=$(cat "$state/loaded")
    [ "$left" -gt 0 ] || exit 113
    echo $((left - 1)) > "$state/loaded"
    ;;
esac
"""


class StopService(unittest.TestCase):
    def setUp(self):
        self.tmp = tempfile.TemporaryDirectory()
        self.addCleanup(self.tmp.cleanup)
        self.dir = Path(self.tmp.name)
        launchctl = self.dir / "launchctl"
        launchctl.write_text(FAKE_LAUNCHCTL)
        launchctl.chmod(launchctl.stat().st_mode | stat.S_IXUSR)

    def stop(self, loaded_queries, label="one.atenea.network.00000001", tries=None):
        (self.dir / "loaded").write_text(str(loaded_queries))
        env = dict(os.environ, PATH=f"{self.dir}:{os.environ['PATH']}")
        if tries is not None:
            env["STOP_SERVICE_TRIES"] = str(tries)
        return subprocess.run(["/bin/sh", "-c", f'. "{HELPER}" && stop_service "{label}"'],
                              env=env, capture_output=True, text=True, timeout=30)

    def lines(self, name):
        path = self.dir / name
        return path.read_text().splitlines() if path.exists() else []

    def test_it_boots_out_the_service_and_waits_until_it_is_unloaded(self):
        result = self.stop(3)
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertEqual(self.lines("bootout.log"), ["system/one.atenea.network.00000001"])
        # 3 consultas con el servicio cargado y una última en la que ya no está.
        self.assertEqual(len(self.lines("print.log")), 4)

    def test_an_already_unloaded_service_returns_at_once(self):
        result = self.stop(0)
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertEqual(len(self.lines("print.log")), 1)

    def test_a_service_that_never_unloads_warns_but_does_not_abort_the_uninstaller(self):
        # El desinstalador usa `set -e`: un fallo aquí lo dejaría a medias.
        result = self.stop(10**6, tries=3)
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertIn("sigue cargado tras bootout", result.stderr)


if __name__ == "__main__":
    unittest.main()
