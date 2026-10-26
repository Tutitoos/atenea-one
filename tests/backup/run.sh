#!/bin/sh
# Copyright 2026 Tutitoos
# SPDX-License-Identifier: Apache-2.0
set -eu
cd "$(dirname "$0")/../.."
# Python estándar solo orquesta Docker; PostgreSQL, age, pgBackRest y Atenea corren en contenedores.
exec python3 tests/backup/run.py
