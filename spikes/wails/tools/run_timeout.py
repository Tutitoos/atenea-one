#!/usr/bin/env python3
# Copyright 2026 Tutitoos
# SPDX-License-Identifier: Apache-2.0
"""Ejecuta una orden con plazo y vuelca su salida a SPIKE_LOG. Portátil (macOS no trae `timeout`)."""
import os
import subprocess
import sys

timeout = int(sys.argv[1])
cmd = sys.argv[2:]
with open(os.environ["SPIKE_LOG"], "wb") as log:
    proc = subprocess.Popen(cmd, stdout=log, stderr=subprocess.STDOUT)
    try:
        rc = proc.wait(timeout=timeout)
    except subprocess.TimeoutExpired:
        proc.kill()
        proc.wait()
        rc = "killed-by-timeout"
print(f"exit: {rc}")
