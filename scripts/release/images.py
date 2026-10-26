#!/usr/bin/env python3
# Copyright 2026 Tutitoos
# SPDX-License-Identifier: Apache-2.0
"""Exporta archivos Docker y manifiestos OCI originales de imágenes locales."""
import hashlib
import json
import subprocess
import sys
import tarfile
from pathlib import Path
from sbom import emit


def sha(data):
    return 'sha256:' + hashlib.sha256(data).hexdigest()


def export(dist, refs, commit, version):
    images = []
    for service, ref in refs.items():
        info = json.loads(subprocess.check_output(['docker', 'image', 'inspect', ref]))[0]
        assert info['Config']['Labels']['org.opencontainers.image.revision'] == commit
        archive = dist / (service + '.docker.tar')
        subprocess.run(['docker', 'image', 'save', '-o', str(archive), ref], check=True)
        with tarfile.open(archive) as tar:
            def find_manifest(descriptor):
                blob = tar.extractfile('blobs/' + descriptor['digest'].replace(':', '/')).read()
                assert sha(blob) == descriptor['digest']
                document = json.loads(blob)
                if 'config' in document:
                    config_digest = document['config']['digest']
                    config_bytes = tar.extractfile('blobs/' + config_digest.replace(':', '/')).read()
                    assert sha(config_bytes) == config_digest
                    config = json.loads(config_bytes)
                    if (config.get('architecture') == info['Architecture'] and
                            config.get('os') == info['Os'] and
                            config.get('rootfs', {}).get('diff_ids') == info['RootFS']['Layers']):
                        assert config['config']['Labels']['org.opencontainers.image.revision'] == commit
                        return blob
                for child in document.get('manifests', []):
                    found = find_manifest(child)
                    if found is not None:
                        return found
                return None
            index = json.load(tar.extractfile('index.json'))
            manifests = [find_manifest(item) for item in index['manifests']]
            manifest = next(blob for blob in manifests if blob is not None)
        target = dist / (service + '.oci-manifest.json')
        target.write_bytes(manifest)
        sbom = service + '.cdx.json'
        emit('image', ref, dist / sbom, version, commit)
        images.append({'name': service, 'local_reference': ref, 'id': info['Id'], 'commit': commit, 'version': version,
                       'config_digest': json.loads(manifest)['config']['digest'],
                       'manifest': target.name, 'manifest_digest': sha(target.read_bytes()),
                       'registry_digest': None,
                       'archive': archive.name, 'sbom': sbom})
    (dist / 'images.json').write_text(json.dumps(images, sort_keys=True, indent=2) + '\n')


if __name__ == '__main__':
    export(Path(sys.argv[1]), dict(item.split('=', 1) for item in sys.argv[4:]), sys.argv[2], sys.argv[3])
