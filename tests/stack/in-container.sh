#!/usr/bin/env bash
set -euo pipefail
export PATH=/cache/target/release:$PATH
bash /src/tests/stack/lab.sh setup
python3 /src/tests/stack/probe.py
python3 /src/tests/stack/api.py
