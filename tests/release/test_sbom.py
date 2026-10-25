"""SBOM de Rust: solo producción, por raíz, con Wintun y dependencias del .deb."""
import json
import sys
import tempfile
import unittest
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]
sys.path.insert(0, str(ROOT / "scripts" / "release"))
import sbom  # noqa: E402


def pkg(name):
    return {"id": name, "name": name, "version": "1.0.0", "license": "MIT"}


def dep(name, kind):
    return {"pkg": name, "dep_kinds": [{"kind": kind, "target": None}]}


METADATA = {
    "packages": [pkg(n) for n in ("app", "other", "lib", "build-helper", "test-only", "transitive-dev")],
    "workspace_members": ["app", "other"],
    "resolve": {"nodes": [
        {"id": "app", "deps": [dep("lib", None), dep("build-helper", "build"), dep("test-only", "dev")]},
        {"id": "other", "deps": [dep("lib", None)]},
        {"id": "lib", "deps": []},
        {"id": "build-helper", "deps": []},
        {"id": "test-only", "deps": [dep("transitive-dev", None)]},
        {"id": "transitive-dev", "deps": []},
    ]},
}


class RustSbom(unittest.TestCase):
    def build(self, **kwargs):
        with tempfile.TemporaryDirectory() as tmp:
            tmp = Path(tmp)
            (tmp / "meta.json").write_text(json.dumps(METADATA))
            (tmp / "Cargo.lock").write_text('[[package]]\nname = "lib"\nversion = "1.0.0"\nchecksum = "' + "ab" * 32 + '"\n')
            return sbom.rust(tmp / "meta.json", tmp / "Cargo.lock", **kwargs)

    def test_dev_only_dependencies_are_excluded(self):
        components, deps = self.build()
        names = {c["name"] for c in components}
        self.assertEqual(names, {"app", "other", "lib", "build-helper"})
        self.assertEqual(next(d for d in deps if d["ref"] == "app")["dependsOn"], ["build-helper", "lib"])

    def test_roots_limit_the_graph(self):
        components, _ = self.build(roots=["other"])
        self.assertEqual({c["name"] for c in components}, {"other", "lib"})
        with self.assertRaises(AssertionError):
            self.build(roots=["missing"])

    def test_wintun_and_deb_depends(self):
        components, _ = self.build(wintun_script=ROOT / "packaging/windows/get-wintun.ps1", deb_script=ROOT / "packaging/deb/build.sh")
        by_name = {c["name"]: c for c in components}
        self.assertEqual(by_name["wintun"]["version"], "0.14.1")
        self.assertEqual(len(by_name["wintun"]["hashes"][0]["content"]), 64)
        self.assertEqual(by_name["libc6"]["version"], ">= 2.39")
        self.assertIn("nftables", by_name)


if __name__ == "__main__":
    unittest.main()
