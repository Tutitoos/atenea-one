// Copyright 2026 Tutitoos
// SPDX-License-Identifier: Apache-2.0
import js from '@eslint/js';
import ts from 'typescript-eslint';
import a11y from 'eslint-plugin-jsx-a11y';
export default [{ ignores: ['build/**', '.react-router/**', 'src/**', 'gen/**'] }, js.configs.recommended, ...ts.configs.recommended, { files: ['**/*.{ts,tsx}'], languageOptions: { globals: { window: 'readonly', document: 'readonly', navigator: 'readonly', localStorage: 'readonly', URL: 'readonly', Request: 'readonly', Response: 'readonly', setTimeout: 'readonly', clearTimeout: 'readonly', console: 'readonly' } } }, { files: ['**/*.tsx'], plugins: { 'jsx-a11y': a11y }, rules: a11y.flatConfigs.recommended.rules }, { files: ['scripts/*.mjs'], languageOptions: { globals: { process: 'readonly' } } }];
