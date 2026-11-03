#!/usr/bin/env python3
# Copyright 2026 Tutitoos
# SPDX-License-Identifier: Apache-2.0
"""Informe de licencias de los módulos vendorizados (spike S6 de ADR-019).

Clasifica el texto del fichero de licencia de cada módulo por palabras clave y lo compara con la lista permitida
de `deny.toml`. Es una heurística: no sustituye a una herramienta de licencias; sirve para ver si hay algo que
no encaje y qué módulos no traen fichero de licencia en `vendor/`.

Uso: license_report.py <vendor> <salida>
"""
import json
import os
import re
import sys

# Copia de [licenses].allow de deny.toml (sin las excepciones MPL-2.0 de Tauri, que desaparecen con ADR-019).
ALLOW = {
    "Apache-2.0", "Apache-2.0 WITH LLVM-exception", "MIT", "ISC", "BSD-2-Clause", "BSD-3-Clause",
    "Unicode-3.0", "Zlib", "CDLA-Permissive-2.0", "BSL-1.0", "Unlicense",
}
LICENSE_FILE = re.compile(r"^(LICEN[CS]E|COPYING|UNLICENSE)", re.I)


def classify(text: str) -> str:
    t = " ".join(text.split())
    low = t.lower()
    if "mozilla public license" in low:
        return "MPL-2.0"
    if "gnu lesser general public license" in low:
        return "LGPL"
    if "gnu affero general public license" in low:
        return "AGPL"
    if "gnu general public license" in low:
        return "GPL"
    if "apache license" in low and "version 2.0" in low:
        return "Apache-2.0"
    if "permission is hereby granted, free of charge" in low:
        return "MIT"
    if "permission to use, copy, modify, and/or distribute" in low or "permission to use, copy, modify, and distribute" in low:
        return "ISC"
    if "redistribution and use in source and binary forms" in low:
        return "BSD-3-Clause" if ("neither the name" in low or "may be used to endorse" in low) else "BSD-2-Clause"
    if "free and unencumbered software" in low:
        return "Unlicense"
    if "altered source versions" in low:
        return "Zlib"
    if "unicode" in low and "terms of use" in low:
        return "Unicode-3.0"
    return "UNKNOWN"


def modules(vendor: str):
    out = []
    with open(os.path.join(vendor, "modules.txt"), encoding="utf-8") as f:
        for line in f:
            if line.startswith("# ") and not line.startswith("## "):
                parts = line[2:].split()
                out.append((parts[0], parts[1] if len(parts) > 1 else ""))
    return out


def main(vendor: str, outdir: str) -> int:
    rows = []
    for path, version in modules(vendor):
        d = os.path.join(vendor, *path.split("/"))
        files = sorted(n for n in (os.listdir(d) if os.path.isdir(d) else []) if LICENSE_FILE.match(n))
        found = []
        for n in files:
            with open(os.path.join(d, n), encoding="utf-8", errors="replace") as fh:
                found.append(classify(fh.read()))
        lic = found[0] if found else "SIN-FICHERO"
        rows.append({"module": path, "version": version, "files": files, "license": lic, "allowed": lic in ALLOW})
    bad = [r for r in rows if not r["allowed"]]
    summary = {
        "total": len(rows),
        "allowed": len(rows) - len(bad),
        "not_allowed": [{"module": r["module"], "license": r["license"]} for r in bad],
    }
    os.makedirs(outdir, exist_ok=True)
    with open(os.path.join(outdir, "licenses.json"), "w", encoding="utf-8") as f:
        json.dump({"summary": summary, "modules": rows}, f, indent=2)
    with open(os.path.join(outdir, "licenses.md"), "w", encoding="utf-8") as f:
        f.write(f"# Licencias de los módulos vendorizados\n\n{summary['allowed']} de {summary['total']} dentro de la lista permitida.\n\n")
        f.write("| Módulo | Versión | Licencia | Permitida |\n| --- | --- | --- | --- |\n")
        for r in rows:
            f.write(f"| {r['module']} | {r['version']} | {r['license']} | {'sí' if r['allowed'] else '**NO**'} |\n")
    print(json.dumps(summary, indent=2))
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv[1], sys.argv[2]))
