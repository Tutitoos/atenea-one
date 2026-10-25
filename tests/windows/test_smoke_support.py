"""Pruebas locales sin SCM, PowerShell ni objetos nativos de Windows."""
import io
import subprocess
import unittest
from contextlib import redirect_stderr
from unittest.mock import patch

import smoke_support as support


class SmokeSupportTests(unittest.TestCase):
    def test_windows_powershell_environment_drops_pwsh_modules_case_insensitively(self):
        source = {'SystemRoot': r'D:\Windows', 'PSMODULEPATH': r'C:\Program Files\PowerShell\7\Modules',
                  'pshome': 'pwsh', 'PowerShell_Distribution_Channel': 'MSI', 'OTHER': 'keep'}
        result = support.powershell_environment(source)
        self.assertEqual(result, {'SystemRoot': r'D:\Windows', 'OTHER': 'keep',
                                 'PSModulePath': r'D:\Windows\System32\WindowsPowerShell\v1.0\Modules'})
        self.assertEqual(source['pshome'], 'pwsh')
        self.assertEqual(support.powershell_executable(source),
                         r'D:\Windows\System32\WindowsPowerShell\v1.0\powershell.exe')

    @patch('smoke_support.subprocess.run')
    def test_failed_command_keeps_diagnostic_output(self, run):
        run.return_value = subprocess.CompletedProcess(['command'], 1, 'SCM failure detail')
        errors = io.StringIO()
        with redirect_stderr(errors), self.assertRaises(subprocess.CalledProcessError) as raised:
            support.call('command')
        self.assertIsInstance(raised.exception, subprocess.SubprocessError)
        self.assertEqual(raised.exception.output, 'SCM failure detail')
        self.assertIn('SCM failure detail', errors.getvalue())
        self.assertEqual(run.call_args.kwargs['timeout'], 30)

    @patch('smoke_support.subprocess.run')
    def test_timeout_keeps_partial_diagnostics(self, run):
        run.side_effect = subprocess.TimeoutExpired(['command'], 1, output=b'agent.log partial')
        errors = io.StringIO()
        with redirect_stderr(errors), self.assertRaises(subprocess.TimeoutExpired):
            support.call('command', timeout=1)
        self.assertIn('agent.log partial', errors.getvalue())

    @patch('smoke_support.call', return_value='{"dns":"none"}')
    def test_pipe_is_in_bounded_child(self, call):
        self.assertEqual(support.ask(r'\\.\pipe\atenea-status', 'status'), {'dns': 'none'})
        self.assertEqual(call.call_args.args[-2:], (r'\\.\pipe\atenea-status', 'status'))
        self.assertEqual(call.call_args.kwargs['timeout'], 5)

    @patch('builtins.open', side_effect=FileNotFoundError(2, 'pipe aún ausente'))
    def test_pipe_readiness_absence_is_not_a_diagnostic_failure(self, open_pipe):
        self.assertIsNone(support.pipe_exchange(r'\\.\pipe\atenea-control', 'status'))

    @patch('builtins.open')
    def test_pipe_readiness_busy_is_expected_but_access_denied_is_not(self, open_pipe):
        busy = OSError('pipe ocupado')
        busy.winerror = 231
        open_pipe.side_effect = busy
        self.assertIsNone(support.pipe_exchange(r'\\.\pipe\atenea-control', 'status'))
        open_pipe.side_effect = PermissionError(13, 'acceso denegado')
        with self.assertRaises(PermissionError):
            support.pipe_exchange(r'\\.\pipe\atenea-control', 'status')

    def test_projected_nrpt_normalizes_scalar_and_array_properties(self):
        scalar = '{"Name":"regla","Namespace":".lab.internal","NameServers":"192.0.2.53","Comment":"Atenea prueba"}'
        rule = support.parse_nrpt(scalar)[0]
        self.assertEqual(rule['Namespace'], ['.lab.internal'])
        self.assertEqual(rule['NameServers'], ['192.0.2.53'])
        arrays = '[{"Name":"regla","Namespace":[".lab.internal"],"NameServers":["192.0.2.53","2001:db8::53"],"Comment":"Atenea prueba"}]'
        rule = support.parse_nrpt(arrays)[0]
        self.assertEqual(rule['NameServers'], ['192.0.2.53', '2001:db8::53'])
        self.assertEqual(rule['Namespace'], ['.lab.internal'])

    def test_projected_nrpt_empty_snapshot_has_no_rules(self):
        for text in ['', '[]', 'null']:
            self.assertEqual(support.parse_nrpt(text), [])

    def test_projected_nrpt_rejects_cim_wrappers_without_dumping_them(self):
        for text in ['"incorrecto"', '{"NameServers":{"CimClass":"objeto no proyectado"}}']:
            with self.assertRaisesRegex(ValueError, '^NRPT: '):
                support.parse_nrpt(text)


if __name__ == '__main__':
    unittest.main()
