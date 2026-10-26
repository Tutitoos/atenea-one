#!/usr/bin/env python3
# Copyright 2026 Tutitoos
# SPDX-License-Identifier: Apache-2.0
"""Solo invocado por el job publish aprobado; efectúa escrituras en el registro."""
import json
import subprocess
import sys
from pathlib import Path

path = Path(sys.argv[1]) / 'images.json'
images = json.loads(path.read_text())
for image in images:
    subprocess.run(['docker', 'load', '-i', str(path.parent / image['archive'])], check=True)
    target = sys.argv[2] + '/' + image['name'] + ':' + image['commit']
    subprocess.run(['docker', 'tag', image['id'], target], check=True)
    subprocess.run(['docker', 'push', target], check=True)
    info = json.loads(subprocess.check_output(['docker', 'image', 'inspect', target]))[0]
    repository = target.rsplit(':', 1)[0]
    image['registry_digest'] = next(ref for ref in info['RepoDigests'] if ref.startswith(repository + '@sha256:'))
path.write_text(json.dumps(images, sort_keys=True, indent=2) + '\n')
