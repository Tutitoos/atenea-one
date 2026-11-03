// Copyright 2026 Tutitoos
// SPDX-License-Identifier: Apache-2.0
import type { Config } from '@react-router/dev/config';
// SPA estática: el binario de Go la embebe; no hay servidor.
export default { ssr: false } satisfies Config;
