"""Pruebas locales sin SCM, PowerShell ni objetos nativos de Windows."""
import io
import json
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

    def test_nrpt_scalar_list_and_ps51_wrappers(self):
        rule = {'Name': 'regla', 'Namespace': ['.lab.internal'],
                'NameServers': ['192.0.2.53', '2001:db8::53'], 'Comment': 'Atenea prueba'}
        for value in [rule, [rule], {'value': [rule], 'Count': 1},
                      {'Value': rule, 'Count': 1}, {'value': [rule]}]:
            with self.subTest(value=value):
                self.assertEqual(support.parse_nrpt(json.dumps(value)), [rule])

    def test_nrpt_normalizes_nested_string_collections(self):
        for value, expected in [
                ('.lab.internal', ['.lab.internal']),
                (['.lab.internal'], ['.lab.internal']),
                ({'value': ['.lab.internal'], 'Count': 1}, ['.lab.internal']),
                ({'Value': '.lab.internal', 'Count': 1}, ['.lab.internal']),
                (None, []), ([], []), ({'value': None, 'Count': 0}, [])]:
            rule = {'Name': 'regla', 'Namespace': value, 'NameServers': value, 'Comment': ''}
            with self.subTest(value=value):
                result = support.parse_nrpt(json.dumps(rule))[0]
                self.assertEqual(result['Namespace'], expected)
                self.assertEqual(result['NameServers'], expected)

    def test_empty_collections(self):
        for parse in (support.parse_nrpt, support.parse_strings):
            for text in ['', '  ', '[]', 'null', '{"value":[],"Count":0}',
                         '{"Value":null,"Count":0}']:
                with self.subTest(parser=parse.__name__, text=text):
                    self.assertEqual(parse(text), [])

    def test_string_scalar_list_and_ps51_wrappers(self):
        for value in ['atenea0', ['atenea0'], {'value': ['atenea0'], 'Count': 1},
                      {'Value': 'atenea0', 'Count': 1}]:
            with self.subTest(value=value):
                self.assertEqual(support.parse_strings(json.dumps(value)), ['atenea0'])
        self.assertEqual(support.parse_strings('["10.77.0.2","fd77::2"]'),
                         ['10.77.0.2', 'fd77::2'])
        self.assertEqual(support.parse_strings('""'), [''])

    def test_snapshot_typed_records_and_collection_shapes(self):
        route = {'DestinationPrefix': '0.0.0.0/0', 'NextHop': '192.0.2.1',
                 'InterfaceIndex': 7, 'RouteMetric': 0}
        dns = {'InterfaceIndex': 7, 'AddressFamily': 2,
               'ServerAddresses': ['192.0.2.53']}
        for wrap in (lambda value: value, lambda value: [value],
                     lambda value: {'value': [value], 'Count': 1},
                     lambda value: {'Value': value, 'Count': 1}):
            value = {'routes': wrap(route), 'dns': wrap(dns)}
            with self.subTest(value=value):
                self.assertEqual(support.parse_snapshot(json.dumps(value)),
                                 {'routes': [route], 'dns': [dns]})
        for addresses, expected in [
                ('2001:db8::53', ['2001:db8::53']),
                (['192.0.2.53', '2001:db8::53'], ['192.0.2.53', '2001:db8::53']),
                ({'value': ['2001:db8::53'], 'Count': 1}, ['2001:db8::53']),
                ({'Value': '2001:db8::53', 'Count': 1}, ['2001:db8::53']),
                (None, []), ([], [])]:
            value = {'routes': [], 'dns': dict(dns, AddressFamily=23, ServerAddresses=addresses)}
            with self.subTest(addresses=addresses):
                result = support.parse_snapshot(json.dumps(value))
                self.assertEqual(result['dns'][0]['ServerAddresses'], expected)
                self.assertEqual(result['dns'][0]['AddressFamily'], 23)
        for empty in (None, [], {'value': [], 'Count': 0}):
            self.assertEqual(support.parse_snapshot(json.dumps({'routes': empty, 'dns': empty})),
                             {'routes': [], 'dns': []})

    def test_service_requires_exactly_one_typed_record(self):
        service = {'State': 'Stopped', 'StartMode': 'Manual', 'StartName': 'LocalSystem',
                   'PathName': '"C:\\Program Files\\Atenea\\atenea-agent.exe" run --service'}
        for value in [service, [service], {'value': [service], 'Count': 1},
                      {'Value': service, 'Count': 1}]:
            with self.subTest(value=value):
                self.assertEqual(support.parse_service(json.dumps(value)), service)
        for value in [None, [], [service, service], {}, dict(service, State=None),
                      dict(service, StartMode=1), dict(service, StartName=[]),
                      dict(service, PathName={'CimClass': 'objeto no proyectado'})]:
            with self.subTest(value=value), self.assertRaises(ValueError):
                support.parse_service(json.dumps(value))
        with self.assertRaises(ValueError):
            support.parse_service('')

    def test_count_requires_nonnegative_integer(self):
        for text, expected in [('0', 0), ('20', 20), (' \n123\n', 123)]:
            self.assertEqual(support.parse_count(text), expected)
        for text in ['', 'null', '-1', 'true', 'false', '1.0', '"1"', '[]',
                     '[1]', '{"value":1,"Count":1}', '{}']:
            with self.subTest(text=text), self.assertRaises(ValueError):
                support.parse_count(text)

    def test_collections_reject_invalid_counts_and_unprojected_values(self):
        for parse, record in [
                (support.parse_nrpt, {'Name': 'regla', 'Namespace': [], 'NameServers': [], 'Comment': ''}),
                (support.parse_strings, 'atenea0')]:
            for count in [-1, True, 1.0, '1', None, 0, 2]:
                value = {'value': [record], 'Count': count}
                with self.subTest(parser=parse.__name__, count=count), self.assertRaises(ValueError):
                    parse(json.dumps(value))
            for value in [42, False, [['atenea0']], {'CimClass': 'objeto no proyectado'},
                          {'value': [record], 'Value': [record], 'Count': 1},
                          {'value': [record], 'Count': 1, 'CimClass': 'objeto no proyectado'},
                          {'Count': 0}]:
                with self.subTest(parser=parse.__name__, value=value), self.assertRaises(ValueError):
                    parse(json.dumps(value))

    def test_records_reject_missing_fields_and_unprojected_nested_values(self):
        rule = {'Name': 'regla', 'Namespace': [], 'NameServers': [], 'Comment': ''}
        for field in rule:
            value = dict(rule)
            del value[field]
            with self.subTest(field=field), self.assertRaises(ValueError):
                support.parse_nrpt(json.dumps(value))
        for field, bad in [('Name', 1), ('Comment', None),
                           ('Namespace', {'CimClass': 'objeto no proyectado'}),
                           ('NameServers', [53]),
                           ('Namespace', {'value': ['.lab'], 'Count': 0})]:
            with self.subTest(field=field), self.assertRaises(ValueError):
                support.parse_nrpt(json.dumps(dict(rule, **{field: bad})))
        route = {'DestinationPrefix': '::/0', 'NextHop': 'fe80::1',
                 'InterfaceIndex': 7, 'RouteMetric': 10}
        dns = {'InterfaceIndex': 7, 'AddressFamily': 23, 'ServerAddresses': []}
        for name, record in [('routes', route), ('dns', dns)]:
            for field in record:
                value = {name: [record], 'dns' if name == 'routes' else 'routes': []}
                value[name] = [dict(record, **{field: {'value__': 23}})]
                with self.subTest(name=name, field=field), self.assertRaises(ValueError):
                    support.parse_snapshot(json.dumps(value))
            value = {'routes': [], 'dns': []}
            value[name] = {'value': [record], 'Count': 0}
            with self.subTest(name=name), self.assertRaises(ValueError):
                support.parse_snapshot(json.dumps(value))
        for value in [None, [], {}, {'routes': []}, {'value': {'routes': [], 'dns': []}, 'Count': 1}]:
            with self.subTest(value=value), self.assertRaises(ValueError):
                support.parse_snapshot(json.dumps(value))
        with self.assertRaises(ValueError):
            support.parse_snapshot('')
        for field in ('InterfaceIndex', 'RouteMetric'):
            with self.subTest(field=field), self.assertRaises(ValueError):
                support.parse_snapshot(json.dumps({'routes': dict(route, **{field: True}), 'dns': []}))
        for family in ['IPv6', '23', True, {'value__': 23}]:
            with self.subTest(family=family), self.assertRaises(ValueError):
                support.parse_snapshot(json.dumps({'routes': [], 'dns': dict(dns, AddressFamily=family)}))

    def test_private_principals_reads_only_protected_dacl_allow_aces(self):
        for text in ['D:P(A;;FA;;;SY)(A;;FA;;;BA)',
                     'O:SYG:SYD:PAI(A;;FA;;;SY)(A;;FA;;;BA)(A;;FR;;;SY)',
                     'D:P(D;;FA;;;WD)(A;;FA;;;SY)(A;;FA;;;BA)S:(AU;SA;FA;;;WD)']:
            with self.subTest(text=text):
                self.assertEqual(support.parse_private_principals(text), {'SY', 'BA'})
        self.assertEqual(support.parse_private_principals('D:P'), set())
        self.assertEqual(support.parse_private_principals('D:P(A;;FA;;;S-1-5-18)'),
                         {'S-1-5-18'})
        for text in ['', 'null', '"D:P(A;;FA;;;SY)"', 'D:(A;;FA;;;SY)',
                     'D:AI(A;;FA;;;SY)', 'D:PARX(A;;FA;;;SY)', 'S:(A;;FA;;;SY)',
                     'D:P(', 'D:P(A;;FA;;;SY', 'D:P(A;;FA;;;SY))',
                     'D:P(A;;FA;;;)', 'D:P(A;;SY)', 'D:P(unknown;;FA;;;SY)']:
            with self.subTest(text=text), self.assertRaises(ValueError):
                support.parse_private_principals(text)

    def test_all_parser_errors_include_bounded_original_output(self):
        parsers = (support.parse_nrpt, support.parse_snapshot, support.parse_service,
                   support.parse_strings, support.parse_count, support.parse_private_principals)
        for parse in parsers:
            for text in ['invalid-json', '{"incorrecto":"' + 'x' * 1200 + '"}',
                         '{"truncado":"' + 'x' * 1200]:
                with self.subTest(parser=parse.__name__, text=text[:20]):
                    with self.assertRaises(ValueError) as raised:
                        parse(text)
                    message = str(raised.exception)
                    self.assertIn(text[:1000], message)
                    if len(text) > 1000:
                        self.assertNotIn(text[:1001], message)
                    self.assertLess(len(message), 1200)


if __name__ == '__main__':
    unittest.main()
