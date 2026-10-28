// Copyright 2026 Tutitoos
// SPDX-License-Identifier: Apache-2.0
import { type RouteConfig, index, layout, route } from '@react-router/dev/routes';

export default [
  layout('routes/shell.tsx', [
    index('routes/home.tsx'),
    route('devices', 'routes/devices.tsx', [index('routes/devices-index.tsx'), route(':id', 'routes/device.tsx')]),
    route('networks', 'routes/networks.tsx'),
    route('servers', 'routes/servers.tsx'),
    route('servers/:host', 'routes/server.tsx'),
    route('diagnostics', 'routes/diagnostics.tsx'),
  ]),
] satisfies RouteConfig;
