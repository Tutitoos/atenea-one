#!/usr/bin/env python3
# Copyright 2026 Tutitoos
# SPDX-License-Identifier: Apache-2.0
"""Solo invocado por el job publish aprobado; efectúa escrituras en el registro.

Uso: publish-images.py DIST PREFIJO SALIDA. No toca dist/ (images.json debe seguir siendo el del artefacto sin firma):
los digests de registro van a SALIDA, que `manifest.py create --registry-digests` incorpora al manifiesto firmado.
"""
import json
import subprocess
import sys
from pathlib import Path

path = Path(sys.argv[1]) / 'images.json'
images = json.loads(path.read_text())
digests = {}
for image in images:
    subprocess.run(['docker', 'load', '-i', str(path.parent / image['archive'])], check=True)
    target = sys.argv[2] + '/' + image['name'] + ':' + image['commit']
    subprocess.run(['docker', 'tag', image['id'], target], check=True)
    subprocess.run(['docker', 'push', target], check=True)
    info = json.loads(subprocess.check_output(['docker', 'image', 'inspect', target]))[0]
    repository = target.rsplit(':', 1)[0]
    digests[image['name']] = next(ref for ref in info['RepoDigests'] if ref.startswith(repository + '@sha256:'))
Path(sys.argv[3]).write_text(json.dumps(digests, sort_keys=True, indent=2) + '\n')
