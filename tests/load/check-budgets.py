#!/usr/bin/env python3
# Copyright 2026 Tutitoos
# SPDX-License-Identifier: Apache-2.0
"""Comprueba resultados reales JSONL; nunca convierte ausencia de datos en verde."""
import argparse
import json
import math
from pathlib import Path
import sys


def nested(row, path):
    for part in path.split('.'):
        row = row[part]
    if isinstance(row, bool) or not isinstance(row, (int, float)) or not math.isfinite(row):
        raise ValueError('Métrica no numérica/finita: '+path)
    return row


def check(rows, budgets):
    failures = []
    for budget in budgets:
        matching = [row for row in rows if all(row.get(key) == value for key,value in budget['match'].items())]
        if len(matching) < budget.get('samples', 2):
            failures.append(budget['id']+': faltan muestras ('+str(len(matching))+')')
            continue
        for row in matching:
            try:
                for metric, expected in budget.get('require', {}).items():
                    value = row
                    for part in metric.split('.'):
                        value = value[part]
                    if type(value) is not type(expected) or value != expected:
                        failures.append(f"{budget['id']}: {metric} no cumple {expected}")
                if row.get('kind') == 'soak-stack-summary':
                    if nested(row, 'steady_duration_s') > 300 and nested(row, 'steady_renewal_ok') < 1:
                        failures.append(budget['id']+': faltan renovaciones relay OK en estado estable')
                for metric, maximum in budget.get('max', {}).items():
                    value = nested(row, metric)
                    if value > maximum:
                        failures.append(f"{budget['id']}: {metric}={value} > {maximum}")
                for metric, minimum in budget.get('min', {}).items():
                    value = nested(row, metric)
                    if value < minimum:
                        failures.append(f"{budget['id']}: {metric}={value} < {minimum}")
                for name, count in budget.get('process_count', {}).items():
                    if sum(p.get('name')==name for p in row.get('resources', {}).values()) < count:
                        failures.append(f"{budget['id']}: faltan procesos {name}")
                for process in row.get('resources', {}).values():
                    for metric, maximum in budget.get('process_max', {}).get(process['name'], {}).items():
                        if nested(process, metric) > maximum:
                            failures.append(f"{budget['id']}: {process['name']}.{metric} > {maximum}")
            except (KeyError,ValueError,TypeError) as error:
                failures.append(budget['id']+': dato requerido inválido '+str(error))
    return failures


def self_check():
    budget=[dict(id='prueba', match=dict(kind='tcp'), samples=2, max=dict(elapsed_s=2), min=dict(mbit_s=1))]
    good=dict(kind='tcp',elapsed_s=1,mbit_s=2)
    assert not check([good,good],budget)
    assert check([],budget)
    assert check([good,dict(good,elapsed_s=3)],budget)
    assert check([good,dict(good,mbit_s=0)],budget)
    assert check([good,dict(good,elapsed_s=float('nan'))],budget)
    assert check([good,dict(kind='tcp')],budget)
    resource=[dict(id='recurso',match=dict(kind='tcp'),samples=2,process_count={'agent':1})]
    assert check([good,good],resource)
    required=[dict(id='booleano',match=dict(kind='tcp'),samples=2,require={'ok':True})]
    assert check([dict(good,ok=True),dict(good,ok=False)],required)
    assert not check([dict(good,ok=True),dict(good,ok=True)],required)


def main():
    parser=argparse.ArgumentParser(description=__doc__)
    parser.add_argument('results',nargs='*',type=Path)
    parser.add_argument('--budgets',type=Path,default=Path(__file__).with_name('budgets.json'))
    parser.add_argument('--self-check',action='store_true')
    args=parser.parse_args()
    if args.self_check:
        self_check();print('OK self-check: falta/exceso/inferior/NaN/dato inválido');return
    if not args.results:
        parser.error('especifique ficheros JSONL de la ejecución')
    rows=[]
    for path in args.results:
        for line in path.read_text().splitlines():
            if line.strip():rows.append(json.loads(line))
    failures=check(rows,json.loads(args.budgets.read_text())['budgets'])
    for failure in failures:print('FAIL '+failure,file=sys.stderr)
    if failures:raise SystemExit(1)
    print('OK presupuestos locales Docker/Mac arm64; no dimensiona VPS')

if __name__=='__main__':main()
