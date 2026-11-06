# Copyright 2026 Tutitoos
# SPDX-License-Identifier: Apache-2.0
"""images.json del artefacto sin firma debe seguir verificando tras publicar; los digests de registro solo van al manifiesto firmado."""
import json
import os
import subprocess
import sys
import tempfile
import unittest
from pathlib import Path

SCRIPTS = Path(__file__).resolve().parents[2] / "scripts/release"
sys.path.insert(0, str(SCRIPTS))
import manifest  # noqa: E402

COMMIT = "a" * 40
VERSION = "0.0.1"
DIGEST = "ghcr.example.org/atenea/postgres-backup@sha256:" + "b" * 64


class ImagesJson(unittest.TestCase):
    def setUp(self):
        self.tmp = tempfile.TemporaryDirectory()
        self.addCleanup(self.tmp.cleanup)
        tmp = Path(self.tmp.name)
        self.dist = tmp / "dist"
        self.dist.mkdir()
        sbom = {"bomFormat": "CycloneDX", "specVersion": "1.5", "version": 1, "components": [], "dependencies": [],
                "metadata": {"component": {"version": VERSION}, "properties": [{"name": "atenea:commit", "value": COMMIT}]}}
        files = {
            "BUILD-INFO.json": {"version": VERSION, "commit": COMMIT, "toolchain": "rustc 0.0.0"},
            "postgres-backup.cdx.json": sbom,
            "postgres-backup.oci-manifest.json": {"config": {"digest": "sha256:" + "c" * 64}},
        }
        for name, data in files.items():
            (self.dist / name).write_text(json.dumps(data))
        (self.dist / "postgres-backup.docker.tar").write_bytes(b"tar sintetico")
        image = {"archive": "postgres-backup.docker.tar", "commit": COMMIT, "config_digest": "sha256:" + "c" * 64,
                 "id": "sha256:" + "d" * 64, "local_reference": "atenea-release/postgres-backup:build",
                 "manifest": "postgres-backup.oci-manifest.json", "manifest_digest": "sha256:" + manifest.digest(self.dist / "postgres-backup.oci-manifest.json"),
                 "name": "postgres-backup", "registry_digest": None, "sbom": "postgres-backup.cdx.json", "version": VERSION}
        (self.dist / "images.json").write_text(json.dumps([image], sort_keys=True, indent=2) + "\n")
        self.unsigned = (self.dist / "images.json").read_bytes()
        self.key, self.pub = str(tmp / "k.key"), str(tmp / "k.pub")
        subprocess.run(["openssl", "genpkey", "-algorithm", "ED25519", "-out", self.key], check=True)
        subprocess.run(["openssl", "pkey", "-in", self.key, "-pubout", "-out", self.pub], check=True)
        os.environ["SOURCE_DATE_EPOCH"] = "0"

    def verify(self):
        return subprocess.run([sys.executable, str(SCRIPTS / "manifest.py"), "verify", str(self.dist), self.pub, COMMIT, VERSION],
                              stdout=subprocess.PIPE, stderr=subprocess.PIPE)

    def test_published_release_verifies_with_unsigned_images_json(self):
        manifest.create(self.dist, self.key, VERSION, COMMIT, {"postgres-backup": DIGEST})
        self.assertEqual((self.dist / "images.json").read_bytes(), self.unsigned)
        self.assertEqual(json.loads((self.dist / "release-manifest.json").read_text())["images"][0]["registry_digest"], DIGEST)
        self.assertEqual(self.verify().returncode, 0)

    def test_images_json_with_registry_digest_is_rejected(self):
        manifest.create(self.dist, self.key, VERSION, COMMIT, {"postgres-backup": DIGEST})
        images = json.loads(self.unsigned)
        images[0]["registry_digest"] = DIGEST
        (self.dist / "images.json").write_text(json.dumps(images, sort_keys=True, indent=2) + "\n")
        self.assertNotEqual(self.verify().returncode, 0)

    def test_tampered_registry_digest_is_rejected(self):
        manifest.create(self.dist, self.key, VERSION, COMMIT, {"postgres-backup": DIGEST})
        target = self.dist / "release-manifest.json"
        target.write_text(target.read_text().replace("b" * 64, "e" * 64))
        self.assertNotEqual(self.verify().returncode, 0)


if __name__ == "__main__":
    unittest.main()
