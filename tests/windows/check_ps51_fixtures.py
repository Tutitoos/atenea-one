# Copyright 2026 Tutitoos
# SPDX-License-Identifier: Apache-2.0
"""Comprueba muestras recogidas por Windows PowerShell 5.1 antes del smoke nativo."""
import argparse
import json
from pathlib import Path

from smoke_support import (
    parse_count,
    parse_nrpt,
    parse_private_principals,
    parse_service,
    parse_snapshot,
    parse_strings,
)


def main():
    arguments = argparse.ArgumentParser(description=__doc__)
    arguments.add_argument('fixtures', type=Path)
    args = arguments.parse_args()
    document = json.loads(args.fixtures.read_text(encoding='utf-8-sig'))
    version = document['realPSversion']
    if not isinstance(version, str) or not version.startswith('5.1.'):
        raise ValueError(f'Las muestras no proceden de Windows PowerShell 5.1: {version!r}')
    parsers = {
        'nrpt': parse_nrpt,
        'snapshot': parse_snapshot,
        'service': parse_service,
        'strings': parse_strings,
        'count': parse_count,
        'sddl': parse_private_principals,
    }
    cases = document['cases']
    if not isinstance(cases, list) or not cases:
        raise ValueError('El archivo nativo no contiene casos')
    covered = set()
    for case in cases:
        parser = case['parser']
        raw = case['json']
        name = case['name']
        expected = json.loads(case['expected'])
        print(f'PS{version} {parser}/{name}: {raw[:1000]}', flush=True)
        try:
            # El lector de SDDL consume texto; los otros lectores consumen JSON.
            source = json.loads(raw) if parser == 'sddl' else raw
            actual = parsers[parser](source)
            if parser == 'sddl':
                actual = sorted(actual)
            if actual != expected:
                raise AssertionError(f'esperado={expected!r}; recibido={actual!r}')
        except Exception as error:
            raise AssertionError(f'PS{version} {parser}/{name}') from error
        covered.add(parser)
    missing = parsers.keys() - covered
    if missing:
        raise AssertionError(f'Faltan lectores en las muestras nativas: {sorted(missing)}')
    print(f'{len(cases)} muestras nativas PS{version} comprobadas en los seis lectores', flush=True)


if __name__ == '__main__':
    main()
