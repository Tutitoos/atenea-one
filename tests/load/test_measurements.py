#!/usr/bin/env python3
# Copyright 2026 Tutitoos
# SPDX-License-Identifier: Apache-2.0
"""Pruebas de unidades de medida, sin Docker, red ni dependencias externas."""
from pathlib import Path
import sys
import unittest
from unittest.mock import patch
sys.path.insert(0,str(Path(__file__).resolve().parent))
import summarize
import control
import traffic
import importlib.util
spec=importlib.util.spec_from_file_location('check_budgets',Path(__file__).with_name('check-budgets.py'))
budgets=importlib.util.module_from_spec(spec)
spec.loader.exec_module(budgets)
timeout_spec=importlib.util.spec_from_file_location('load_timeout',Path(__file__).with_name('timeout.py'))
load_timeout=importlib.util.module_from_spec(timeout_spec)
timeout_spec.loader.exec_module(load_timeout)


def sample(at,ticks,rss=4096):
    fields=['S']+['0']*20
    fields[11]=str(ticks)
    return dict(service='coordinator',pid=1,name='atenea-coordina',sample_unix=at,
                clock_ticks=100,stat='1 (atenea-coordina) '+' '.join(fields),status=f'VmRSS:\t{rss} kB\n',fds=12)


class Measurements(unittest.TestCase):
    def test_steady_counts_only_deltas_and_separates_blocked_direct_probes(self):
        agents = lambda counters: {n:dict(counters=counters, peers=[{'path':'relay-udp'}]) for n in ('a','b')}
        mark = dict(kind='soak-steady', steady_since=10, expected_blocked_path='direct',
            warmup_counters={n:{} for n in ('a','b')}, agent_states=agents({'tx_bad_source':1, 'relay_ready':1}))
        sample = dict(kind='soak-sample', wall_unix=20,
            agent_states=agents({}), agent_counters={n:dict(tx_bad_source=1, probe_failed=2, path_lost=1, relay_ready=1) for n in ('a','b')})
        result = summarize.steady_counters([mark, sample])
        self.assertTrue(result['steady_ok'])
        self.assertEqual(result['warmup_counter_deltas']['a'], {'tx_bad_source':1, 'relay_ready':1})
        self.assertEqual(result['expected_blocked_counter_deltas']['a'], {'probe_failed':2, 'path_lost':1})
        sample['agent_counters']['a']['relay_down'] = 1
        result = summarize.steady_counters([mark, sample])
        self.assertFalse(result['steady_ok'])
        self.assertEqual(result['steady_errors'], 1)
        self.assertFalse(summarize.steady_counters([])['steady_ok'])
        final = dict(kind='soak-summary', wall_unix=730)
        sample['wall_unix'] = 720
        for n in ('a','b'):
            sample['agent_counters'][n].update(relay_down=2, relay_ready=3)
        result = summarize.steady_counters([mark, sample, final])
        self.assertTrue(result['steady_ok'])
        self.assertEqual(result['known_finding'], 'relay-token-cycle')
        sample['agent_counters']['a']['relay_down'] = 3
        self.assertFalse(summarize.steady_counters([mark, sample, final])['steady_ok'])
        sample['agent_counters']['a'].update(relay_down=4, relay_ready=5)
        self.assertFalse(summarize.steady_counters([mark, sample, final])['steady_ok'])

    def test_soak_budget_rejects_missing_or_failed_steady_evidence(self):
        import json
        budget = [r for r in json.loads(Path(__file__).with_name('budgets.json').read_text())['budgets']
                  if r['id'] == 'B24-soak-steady']
        good = dict(kind='soak-stack-summary', steady_ok=True, ok=True, steady_samples=72, steady_errors=0,
            known_finding='relay-token-cycle', steady_duration_s=720, known_finding_limit_per_agent=3,
            known_finding_events={'a':2, 'b':2}, steady_relay_ready={'a':3, 'b':3}, ping_loss_percent=.085)
        self.assertEqual(budgets.check([good], budget), [])
        self.assertTrue(budgets.check([dict(good, steady_errors=1)], budget))
        self.assertTrue(budgets.check([dict(kind='soak-stack-summary', ok=True)], budget))
        self.assertTrue(budgets.check([dict(good, known_finding_events={'a':4,'b':2}, steady_relay_ready={'a':5,'b':3})], budget))
        self.assertTrue(budgets.check([dict(good, known_finding_events={'a':3,'b':2})], budget))
        self.assertTrue(budgets.check([dict(good, ping_loss_percent=1.01)], budget))

    def test_deadline_terminates_process_group_and_waits_for_cleanup(self):
        with patch.object(load_timeout.subprocess, 'Popen') as popen, \
             patch.object(load_timeout.os, 'killpg') as killpg, \
             patch.object(load_timeout.signal, 'signal'):
            child = popen.return_value
            child.pid = 123
            child.wait.side_effect = [load_timeout.subprocess.TimeoutExpired('job', 3), 143]
            self.assertEqual(load_timeout.run(3, ['job']), 124)
            killpg.assert_called_once_with(123, load_timeout.signal.SIGTERM)
            self.assertEqual(child.wait.call_args_list[-1].kwargs, {'timeout': 60})

    def test_soak_repeats_with_injected_clock_until_deadline(self):
        now = [0]
        calls = []
        def iteration(transfer, remaining):
            calls.append((now[0], transfer, remaining))
            now[0] += 2
        failures = traffic.soak_loop(180, iteration, clock=lambda: now[0],
            sleep=lambda seconds: now.__setitem__(0, now[0]+seconds))
        self.assertEqual(failures, [])
        self.assertEqual(now[0], 180)
        self.assertEqual(len(calls), 18)
        self.assertEqual([at for at, transfer, _ in calls if transfer], [0, 60, 120])

    def test_soak_records_iteration_failure_and_keeps_cadence(self):
        now = [0]
        calls, errors = [], []
        def iteration(transfer, remaining):
            calls.append(now[0])
            if now[0] == 60:
                raise RuntimeError('fallo puntual')
        failures = traffic.soak_loop(180, iteration, clock=lambda: now[0],
            sleep=lambda seconds: now.__setitem__(0, now[0]+seconds),
            report=lambda kind, **row: errors.append((kind, row)))
        self.assertEqual(calls, list(range(0, 180, 10)))
        self.assertEqual(now[0], 180)
        self.assertEqual(failures, ['fallo puntual'])
        self.assertEqual(errors[0][0], 'soak-error')

    def test_clock_ticks_not_wall_seconds(self):
        self.assertEqual(summarize.cpu(sample(10,200)),2)
        self.assertEqual(summarize.rss(sample(10,200)),4096)
        slow=sample(10,200);slow['clock_ticks']=1000
        self.assertEqual(summarize.cpu(slow),.2)

    def test_resources_use_real_sample_intervals(self):
        rows=[sample(0,0),sample(10,200,8192),sample(20,300)]
        result=summarize.resources(rows,5,15)['coordinator/1']
        self.assertEqual(result['cpu_percent'],20)
        self.assertEqual(result['rss_peak_kib'],8192)
        self.assertEqual(result['samples'],1)
        self.assertEqual(summarize.resources(rows,30,40),{})

    def test_budget_failure_is_fail_closed(self):
        budgets.self_check()

    def test_json_error_field_is_not_error_severity(self):
        self.assertEqual(summarize.log_error_count('x | {"level":"warn","error":"optional trust install"}'),0)
        self.assertEqual(summarize.log_error_count('x | {"level":"error","msg":"failed"}\nx | ERROR: failed'),2)

    def test_postgres_uri_sets_remote_libpq_parameters(self):
        with patch.dict(control.os.environ,{'DATABASE_URL':'postgres://u:escaped%40value@db.example:5439/atenea'}):
            values=control.pg_env()
        self.assertEqual({k:values[k] for k in ('PGHOST','PGPORT','PGUSER','PGPASSWORD','PGDATABASE')},
            dict(PGHOST='db.example',PGPORT='5439',PGUSER='u',PGPASSWORD='escaped@value',PGDATABASE='atenea'))

if __name__=='__main__':unittest.main()
