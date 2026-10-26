// Copyright 2026 Tutitoos
// SPDX-License-Identifier: Apache-2.0
import { type RouteConfig, index, route } from '@react-router/dev/routes';
export default [
  index('panel.tsx', { id: 'panel-index' }),
  route('*', 'panel.tsx', { id: 'panel' }),
] satisfies RouteConfig;
