#!/usr/bin/env python3
# Copyright 2026 Tutitoos
# SPDX-License-Identifier: Apache-2.0
"""Exige que el environment `release` tenga revisores y solo admita tags `v*`.

No exige «Prevent self-review»: el proyecto tiene un único mantenedor y una segunda
cuenta suya no aportaría separación real (riesgo residual aceptado en ADR-009).

Uso: check-environment.py ENVIRONMENT.json POLICIES.json
(salidas de `gh api repos/R/environments/release` y `.../deployment-branch-policies`).
"""
import json
import sys


def problems(environment, policies):
    found = []
    rules = [r for r in environment.get("protection_rules", []) if r.get("type") == "required_reviewers"]
    if not any(r.get("reviewers") for r in rules):
        found.append("sin revisores obligatorios")
    # «prevent_self_review» no se comprueba: ver arriba.
    if not (environment.get("deployment_branch_policy") or {}).get("custom_branch_policies"):
        found.append("sin restricción de ramas/tags por política personalizada")
    listed = policies.get("branch_policies", [])
    if not listed or any(p.get("type") != "tag" or not p.get("name", "").startswith("v") for p in listed):
        found.append("las políticas deben ser solo de tags v*")
    return found


if __name__ == "__main__":
    with open(sys.argv[1]) as env, open(sys.argv[2]) as pol:
        errors = problems(json.load(env), json.load(pol))
    for error in errors:
        print("environment release: " + error, file=sys.stderr)
    sys.exit(1 if errors else 0)
