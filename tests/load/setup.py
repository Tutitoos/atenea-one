#!/usr/bin/env python3
# Copyright 2026 Tutitoos
# SPDX-License-Identifier: Apache-2.0
"""Reutiliza las funciones del laboratorio sin ejecutar su suite ni compilar."""
import os
from pathlib import Path
import subprocess
import sys
import hashlib
import json
import time

mode = sys.argv[1]
hours = float(sys.argv[2])
assert mode in ('tunnel', 'handshake', 'relay', 'soak')
assert 0 < hours <= 168
metadata = dict(kind='metadata',scenario=mode,wall_unix=time.time(),kernel=list(os.uname()),binaries={})
for binary in ('atenea','atenea-agent','atenea-coordinator','atenea-relay'):
    executable = Path('/cache/target/release') / binary
    if (Path('/load-bin')/binary).exists():
        executable=Path('/load-bin')/binary
    with executable.open('rb') as stream:
        digest = hashlib.file_digest(stream,'sha256').hexdigest()
    metadata['binaries'][binary] = dict(sha256=digest,version=subprocess.check_output([str(executable),'--version'],text=True).strip())
print(json.dumps(metadata),flush=True)
with Path('/results/metadata.jsonl').open('a') as stream:
    stream.write(json.dumps(metadata)+'\n')
source = Path('tests/network-lab/nat/e2e-relay.sh').read_text()
prefix = source.split('# ---------------------------------------------------------------- home: caminos forzados')[0]
prefix = prefix.replace('cargo build --release --locked -q 2>&1 | tail -3', 'test -x /cache/target/release/atenea-agent')
prefix = prefix.replace('export PATH=$CARGO_TARGET_DIR/release:$PATH','export PATH=/load-bin:$CARGO_TARGET_DIR/release:$PATH')
if mode == 'handshake':
    prefix = prefix.replace('VALID=240; REFRESH=20','VALID=3600; REFRESH=20')
if mode == 'relay':
    os.environ['ATENEA_RELAY_MAX_RATE_ENTRIES'] = '128'
prefix += '\nset -e\ntopo_setup home\nstart_services\nwait_path direct 90\n'
if os.environ.get('ATENEA_LOAD_RELAY_CAPACITY_ONLY') != '1':
    prefix += 'python3 /src/tests/load/traffic.py ' + mode + ' ' + str(hours) + '\n'
if mode == 'relay':
    prefix += 'ip netns exec inet python3 /src/tests/load/relay-capacity.py 1\n'
    prefix += 'ip netns exec inet python3 /src/tests/load/relay-capacity.py 2\n'
Path('/tmp/atenea-load-setup.sh').write_text(prefix)
os.environ['PATH'] = '/load-bin:/cache/target/release:' + os.environ['PATH']
code = subprocess.call(['bash', '/tmp/atenea-load-setup.sh'])
import shutil
logs = Path('/results') / ('logs-'+mode+'-'+str(int(time.time())))
logs.mkdir()
for file in Path('/tmp/lab').glob('*.log'):
    shutil.copyfile(file, logs/file.name)
raise SystemExit(code)
