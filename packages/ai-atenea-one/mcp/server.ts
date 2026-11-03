#!/usr/bin/env node
// Copyright 2026 Tutitoos
// SPDX-License-Identifier: Apache-2.0
// El guardián de stdout debe cargarse antes que cualquier otro import: el transporte usa fd 1 para JSON-RPC.
import { sanctionedStdout } from "./stdout-guard.ts";

import { McpServer } from "@modelcontextprotocol/sdk/server/mcp.js";
import { StdioServerTransport } from "@modelcontextprotocol/sdk/server/stdio.js";
import { homedir } from "node:os";
import { Client } from "./client.ts";
import { loadConfig } from "./config.ts";
import { registerTools } from "./tools.ts";

try {
  const server = new McpServer({ name: "atenea-one", version: "0.1.0" }, { capabilities: { tools: {} } });
  registerTools(server, new Client(loadConfig(process.env, homedir())));
  await server.connect(new StdioServerTransport(process.stdin, sanctionedStdout));
} catch (err) {
  // El mensaje de loadConfig/connect nunca incluye el token.
  console.error(`atenea-one: ${err instanceof Error ? err.message : String(err)}`);
  process.exit(1);
}
