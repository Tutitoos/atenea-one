// Copyright 2026 Tutitoos
// SPDX-License-Identifier: Apache-2.0
import { defineConfig } from 'vite';
import { reactRouter } from '@react-router/dev/vite';
import tailwindcss from '@tailwindcss/vite';
export default defineConfig({ plugins: [tailwindcss(), reactRouter()], server: { proxy: { '/api': { target: 'http://127.0.0.1:8080', ws: true }, '/auth': 'http://127.0.0.1:8080' } }, build: { assetsInlineLimit: 0 } });
