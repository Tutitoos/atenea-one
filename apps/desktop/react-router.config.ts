// Copyright 2026 Tutitoos
// SPDX-License-Identifier: Apache-2.0
import type { Config } from '@react-router/dev/config';
// SPA estática: Tauri la embebe en el binario; no hay servidor.
export default { ssr: false } satisfies Config;
