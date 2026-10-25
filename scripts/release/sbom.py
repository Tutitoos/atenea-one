#!/usr/bin/env python3
"""Inventario CycloneDX 1.5 de fuentes y filesystem local, sin red."""
import argparse
import base64
import hashlib
import json
import re
import subprocess
import tarfile
import tempfile
import tomllib
from pathlib import Path


def component(name, version, ref, **extra):
    return {"type": "library", "name": name, "version": version, "bom-ref": ref, **extra}


def validate(bom):
    assert bom["bomFormat"] == "CycloneDX" and bom["specVersion"] == "1.5"
    assert bom["version"] == 1
    refs = {c["bom-ref"] for c in bom["components"]}
    assert len(refs) == len(bom["components"])
    for c in bom["components"]:
        assert c["type"] in ("library", "application", "file")
        assert c["name"] and c["version"] and c["bom-ref"]
        for h in c.get("hashes", []):
            lengths = {"SHA-256": 64, "SHA-512": 128}
            assert len(h["content"]) == lengths[h["alg"]]
            int(h["content"], 16)
    for dep in bom["dependencies"]:
        assert dep["ref"] in refs and set(dep["dependsOn"]) <= refs
    assert bom["metadata"]["component"]["version"]


def rust(metadata, lock):
    data = json.loads(Path(metadata).read_text())
    checksums = {(p["name"], p["version"]): p.get("checksum") for p in tomllib.loads(Path(lock).read_text())["package"]}
    components = []
    for p in data["packages"]:
        c = component(p["name"], p["version"], p["id"])
        checksum = checksums.get((p["name"], p["version"]))
        if checksum:
            c["hashes"] = [{"alg": "SHA-256", "content": checksum}]
        if p.get("license"):
            c["licenses"] = [{"expression": p["license"]}]
        components.append(c)
    deps = [{"ref": n["id"], "dependsOn": n["dependencies"]} for n in data["resolve"]["nodes"]]
    return components, deps


def panel(lock):
    # Parser deliberadamente limitado al lockfile pnpm v9: falla ante otro formato.
    text = Path(lock).read_text()
    assert text.startswith("lockfileVersion: '9.0'")
    sections = {"packages": {}, "snapshots": {}}
    section = None
    key = None
    for line in text.splitlines():
        if not line.strip():
            continue
        if line and not line.startswith(" "):
            section = line.rstrip(":") if line in ("packages:", "snapshots:") else None
        elif section:
            match = re.fullmatch(r"  (\S.*?):(?: \{\})?", line)
            if match:
                key = match[1].strip("'\"")
                sections[section][key] = []
            elif key:
                sections[section][key].append(line)
    components = []
    for key, lines in sections["packages"].items():
        name, version = key.rsplit("@", 1)
        c = component(name, version, "npm:" + key)
        integrity = re.search(r"integrity: (sha(?:256|512))-([A-Za-z0-9+/=]+)", "\n".join(lines))
        if integrity:
            c["hashes"] = [{"alg": "SHA-" + integrity[1][3:], "content": base64.b64decode(integrity[2], validate=True).hex()}]
        components.append(c)
    refs = {c["bom-ref"] for c in components}
    edges = {ref: set() for ref in refs}
    for key, lines in sections["snapshots"].items():
        ref = "npm:" + key.split("(", 1)[0]
        assert ref in refs, key
        active = False
        for line in lines:
            if re.fullmatch(r"    (?:optionalDependencies|dependencies):", line):
                active = True
            elif re.match(r"    \S", line):
                active = False
            elif active:
                m = re.fullmatch(r"      (.+): (.+)", line)
                assert m, line
                target = "npm:" + m[1].strip("'\"") + "@" + m[2].strip("'\"").split("(", 1)[0]
                assert target in refs, target
                edges[ref].add(target)
    assert components
    return components, [{"ref": ref, "dependsOn": sorted(deps)} for ref, deps in sorted(edges.items())]


def image(reference):
    container = subprocess.check_output(["docker", "create", "--network", "none", reference], text=True).strip()
    try:
        with tempfile.TemporaryFile() as output:
            subprocess.run(["docker", "export", container], stdout=output, check=True)
            output.seek(0)
            components = []
            with tarfile.open(fileobj=output) as archive:
                names = {m.name.lstrip("./"): m for m in archive.getmembers()}
                statuses = [member for name, member in names.items() if member.isfile() and
                            (name == "var/lib/dpkg/status" or
                             (name.startswith("var/lib/dpkg/status.d/") and not name.endswith(".md5sums")))]
                if statuses:
                    for member in statuses:
                        status = archive.extractfile(member).read().decode()
                        for entry in status.split("\n\n"):
                            fields = dict(line.split(": ", 1) for line in entry.splitlines() if ": " in line and not line.startswith(" "))
                            if "Package" in fields and fields.get("Status", "install ok installed") == "install ok installed":
                                name, version = fields["Package"], fields["Version"]
                                components.append(component(name, version, "deb:" + name))
                elif "lib/apk/db/installed" in names:
                    status = archive.extractfile(names["lib/apk/db/installed"]).read().decode()
                    for entry in status.split("\n\n"):
                        fields = dict(line.split(":", 1) for line in entry.splitlines() if ":" in line)
                        if "P" in fields:
                            components.append(component(fields["P"], fields["V"], "apk:" + fields["P"]))
                for name, member in names.items():
                    if member.isfile() and name.startswith(("usr/local/bin/", "usr/bin/", "bin/")) and member.mode & 0o111:
                        content = archive.extractfile(member).read()
                        components.append(component(name, "unversioned", "file:" + name, type="file", hashes=[{"alg": "SHA-256", "content": hashlib.sha256(content).hexdigest()}]))
                assert components, "Imagen sin inventario reconocible"
                return components, []
    finally:
        subprocess.run(["docker", "rm", container], check=True, stdout=subprocess.DEVNULL)


def emit(kind, source, output, version, commit, lock="Cargo.lock"):
    if kind == "rust":
        components, dependencies = rust(source, lock)
    elif kind == "panel":
        components, dependencies = panel(source)
    else:
        components, dependencies = image(source)
    bom = {"bomFormat": "CycloneDX", "specVersion": "1.5", "version": 1,
           "metadata": {"component": component("atenea-" + kind, version, "atenea:" + kind, type="application"),
                        "properties": [{"name": "atenea:commit", "value": commit}]},
           "components": sorted(components, key=lambda c: c["bom-ref"]), "dependencies": dependencies}
    validate(bom)
    Path(output).write_text(json.dumps(bom, sort_keys=True, indent=2) + "\n")


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("kind", choices=["rust", "panel", "image"])
    parser.add_argument("source")
    parser.add_argument("output")
    parser.add_argument("--version", required=True)
    parser.add_argument("--commit", required=True)
    parser.add_argument("--lock", default="Cargo.lock")
    args = parser.parse_args()
    emit(args.kind, args.source, args.output, args.version, args.commit, args.lock)
