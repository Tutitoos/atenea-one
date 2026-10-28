# Copyright 2026 Tutitoos
# SPDX-License-Identifier: Apache-2.0
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


class RouteTables(unittest.TestCase):
    V4 = """Routing tables

Internet:
Destination Gateway Flags Netif Expire
default 192.0.2.1 UGScg en0
10.77/24 utun4 USc utun4
10.77.0.128/25 utun4 USc utun4
192.0.2 link#7 UCS en0 !
192.0.2.7 aa:bb:cc:00:00:00 UHLWI en0 100
"""
    V6 = """Routing tables

Internet6:
Destination Gateway Flags Netif Expire
default fe80::1%utun0 UGcIg utun0
::1 ::1 UHL lo0
fd77::/64 link#12 UC utun4
fd77::2 link#12 UHL lo0
fd77:0:0:1::/64 utun4 USc utun4
fe80::1%en0/128 link#7 UCI en0
ff02::%en0/32 link#7 UmCI en0
"""

    def test_darwin_shorthand_ipv4_and_gatewayless_ipv6(self):
        self.assertEqual(smoke.route_interfaces(self.V4, "10.77.0.0/24"), {"utun4"})
        self.assertEqual(smoke.route_interfaces(self.V4, "10.77.0.128/25"), {"utun4"})
        self.assertEqual(smoke.route_interfaces(self.V6, "fd77:0:0:1::/64"), {"utun4"})
        self.assertEqual(smoke.route_interfaces(self.V4, "192.0.2.0/24"), {"en0"})

    def test_defaults_broader_routes_and_host_routes_do_not_prove_exact_prefix(self):
        self.assertEqual(smoke.route_interfaces(self.V6, "fd77:0:0:2::/64"), set())
        self.assertEqual(smoke.route_interfaces(self.V4, "10.77.0.128/26"), set())
        self.assertEqual(smoke.route_interfaces(self.V6, "fd77::2/128"), {"lo0"})
        self.assertEqual(smoke.route_interfaces(self.V6.replace("fd77:0:0:1::/64 utun4 USc", "fd77:0:0:1::/64 utun4 Sc"), "fd77:0:0:1::/64"), set())

    def test_wrong_or_ambiguous_interface_fails_the_assertion(self):
        for table in (self.V6.replace("fd77:0:0:1::/64 utun4 USc utun4", "fd77:0:0:1::/64 link#7 USc en0"),
                      self.V6 + "fd77:0:0:1::/64 link#7 USc en0\n"):
            with mock.patch.object(smoke, "call", return_value=table):
                with self.assertRaises(AssertionError):
                    smoke.check_route("fd77:0:0:1::/64", "utun4")

    def test_route_check_reads_the_correct_numeric_family_table(self):
        with mock.patch.object(smoke, "call", return_value=self.V6) as command:
            with contextlib.redirect_stderr(io.StringIO()):
                smoke.check_route("fd77:0:0:1::/64", "utun4")
            command.assert_called_once_with("/usr/sbin/netstat", "-rn", "-f", "inet6")
        with mock.patch.object(smoke, "call", return_value=self.V4) as command:
            with contextlib.redirect_stderr(io.StringIO()):
                smoke.check_route("10.77.0.128/25", "utun4")
            command.assert_called_once_with("/usr/sbin/netstat", "-rn", "-f", "inet")

    def test_missing_or_malformed_table_fails_closed(self):
        for table in ("", "Destination Gateway Flags Netif\nfd77:0:0:1::/64\n"):
            with self.assertRaises(AssertionError):
                smoke.route_interfaces(table, "fd77:0:0:1::/64")


class NetworkPackaging(unittest.TestCase):
    def test_shell_contract_rejects_ids_and_untrusted_roots_without_privileges(self):
        helper = smoke.REPO / "packaging/macos/atenea-network.sh"
        network = "77777777-7777-4777-8777-777700000001"
        with tempfile.TemporaryDirectory() as directory:
            # Solo aislar stat macOS; se ejercita el parser shell real, sin agente ni launchctl.
            script = '. "$1"; trusted_path() { return 0; }; network_paths "$2" "$3" || exit 78; printf "%s\\n%s\\n" "$config" "$runtime"'
            result = subprocess.run(["/bin/sh", "-c", script, "test", str(helper), network, directory],
                                    capture_output=True, text=True, check=True)
            self.assertEqual(result.stdout.splitlines(), [
                f"{directory}/{network}/device/agent.toml", "/var/run/atenea/00000001"])
            for bad_id, bad_root in ((network[:-1] + "A", directory), ("../" + network, directory),
                                     (network.replace("-", ""), directory), (network, directory + "/"),
                                     (network, directory + "/../x"), (network, "relative")):
                rejected = subprocess.run(["/bin/sh", "-c", script, "test", str(helper), bad_id, bad_root],
                                          capture_output=True)
                self.assertEqual(rejected.returncode, 78)
            untrusted = '. "$1"; trusted_path() { return 1; }; network_paths "$2" "$3"'
            result = subprocess.run(["/bin/sh", "-c", untrusted, "test", str(helper), network, directory])
            self.assertNotEqual(result.returncode, 0)


if __name__ == "__main__":
    unittest.main()
