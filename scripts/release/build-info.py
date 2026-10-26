#!/usr/bin/env python3
# Copyright 2026 Tutitoos
# SPDX-License-Identifier: Apache-2.0
"""Identidad pública del paquete, sin claves ni rutas del compilador."""
import json
import os
print(json.dumps({"commit": os.environ.get("ATENEA_BUILD_COMMIT", "unknown"), "version": os.environ.get("ATENEA_VERSION", "0.1.0")}, sort_keys=True))
