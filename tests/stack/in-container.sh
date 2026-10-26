#!/usr/bin/env bash
# Copyright 2026 Tutitoos
# SPDX-License-Identifier: Apache-2.0
set -euo pipefail
export PATH=/cache/target/release:$PATH
bash /src/tests/stack/lab.sh setup
python3 /src/tests/stack/probe.py
python3 /src/tests/stack/api.py
