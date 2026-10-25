"""Regresiones sin privilegios del supervisor; no ejecutan red ni el agente."""
import contextlib
import io
from pathlib import Path
import subprocess
import sys
import tempfile
import unittest
from unittest import mock

import smoke


class Deadlines(unittest.TestCase):
    def test_command_timeout_is_enforced(self):
        with self.assertRaises(subprocess.TimeoutExpired):
            smoke.run([sys.executable, "-c", "import time; time.sleep(30)"], timeout=0.1)

    def test_default_timeout_covers_calls(self):
        with mock.patch.object(smoke.subprocess, "run") as command:
            smoke.run(["fake"])
            self.assertEqual(command.call_args.kwargs["timeout"], 10)

    def test_readiness_timeout_names_phase(self):
        with contextlib.redirect_stderr(io.StringIO()):
            with self.assertRaisesRegex(AssertionError, "ready simulado"):
                smoke.until(lambda: False, seconds=0, description="ready simulado")

    def test_blocked_command_is_not_retried_as_readiness(self):
        error = subprocess.TimeoutExpired("ifconfig simulado", 0.1)
        with contextlib.redirect_stderr(io.StringIO()):
            with self.assertRaises(subprocess.TimeoutExpired):
                smoke.until(mock.Mock(side_effect=error), description="dirección")

    def test_exited_child_fails_readiness_immediately(self):
        process = mock.Mock(returncode=7)
        process.poll.return_value = 7
        with contextlib.redirect_stderr(io.StringIO()):
            with self.assertRaisesRegex(AssertionError, "proceso terminó con 7"):
                smoke.until(lambda: False, description="socket", process=process)

    def test_background_deadline_kills_process_group(self):
        with tempfile.TemporaryDirectory() as directory, contextlib.redirect_stderr(io.StringIO()):
            work = Path(directory)
            with (work / "peer.log").open("w") as log:
                process, timer = smoke.background(
                    [sys.executable, "-c", "import time; time.sleep(30)"],
                    log, work, "peer", seconds=0.1,
                )
                try:
                    self.assertLess(process.wait(timeout=3), 0)
                    self.assertEqual(int((work / "peer.pid").read_text()), process.pid)
                finally:
                    timer.cancel()
                    smoke.terminate(process)

    def test_diagnostics_always_dumps_both_logs_and_bounds_commands(self):
        with tempfile.TemporaryDirectory() as directory:
            work = Path(directory)
            for name in ("agent", "peer"):
                (work / f"{name}.log").write_text("\x1b[32m" + name + " evidencia\x1b[0m\nfinal completo")
            (work / "agent.toml").write_text("configuración simulada")
            output = io.StringIO()
            result = subprocess.CompletedProcess([], 0, stdout="estado", stderr="")
            with mock.patch.object(smoke, "run", return_value=result) as command:
                with contextlib.redirect_stderr(output):
                    smoke.diagnostics(work)
            self.assertIn("agent evidencia", output.getvalue())
            self.assertIn("peer evidencia", output.getvalue())
            self.assertNotIn("\x1b", output.getvalue())
            self.assertEqual(output.getvalue().count("final completo"), 2)
            self.assertIn("fin log completo agent", output.getvalue())
            self.assertEqual([c.kwargs["timeout"] for c in command.call_args_list], [5, 5, 5, 10])
            self.assertIn("cleanup", command.call_args.args[0])

    def test_echo_reports_actual_result_and_never_counts_failure_as_success(self):
        for code in (0, 2):
            result = subprocess.CompletedProcess([], code, stdout="estadísticas completas", stderr="detalle")
            output = io.StringIO()
            with mock.patch.object(smoke, "run", return_value=result) as command:
                with contextlib.redirect_stderr(output):
                    self.assertEqual(smoke.echo(["/sbin/ping", "-n"]), code == 0)
            self.assertEqual(command.call_args.kwargs["timeout"], 8)
            self.assertIn("estadísticas completas", output.getvalue())
            self.assertIn("detalle", output.getvalue())


if __name__ == "__main__":
    unittest.main()
