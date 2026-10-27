// Copyright 2026 Tutitoos
// SPDX-License-Identifier: Apache-2.0
import { defineConfig } from 'vite';
import { reactRouter } from '@react-router/dev/vite';
import tailwindcss from '@tailwindcss/vite';
export default defineConfig({ plugins: [tailwindcss(), reactRouter()], build: { assetsInlineLimit: 0 } });
