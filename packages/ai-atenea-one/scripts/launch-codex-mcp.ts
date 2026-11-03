// Copyright 2026 Tutitoos
// SPDX-License-Identifier: Apache-2.0
import { spawn, spawnSync } from "node:child_process";
import { once } from "node:events";
import { existsSync } from "node:fs";
import { join } from "node:path";

const target = process.argv[2];
if (!target) {
  console.error("Falta el servidor MCP de destino.");
  process.exit(64);
}

// Codex materializa los plugins locales sin los enlaces de node_modules de pnpm: reinstalar si falta el SDK.
const sdkEntry = join(process.cwd(), "node_modules", "@modelcontextprotocol", "sdk", "dist", "esm", "server", "mcp.js");

if (!existsSync(sdkEntry)) {
  let prepared = false;
  let lastExitCode = 1;
  for (const [command, ...args] of [
    ["pnpm", "install", "--prod"],
    ["npm", "install", "--omit=dev"],
  ]) {
    const install = spawnSync(command, args, { cwd: process.cwd() });
    if (install.error !== undefined) continue; // binario ausente
    // stdout del MCP está reservado a JSON-RPC: la salida de la instalación va a stderr.
    if (install.stdout) process.stderr.write(install.stdout);
    if (install.stderr) process.stderr.write(install.stderr);
    lastExitCode = install.status ?? 1;
    if (lastExitCode === 0 && existsSync(sdkEntry)) {
      prepared = true;
      break;
    }
  }
  if (!prepared) {
    console.error("No se pudieron preparar las dependencias del MCP para Codex.");
    process.exit(lastExitCode);
  }
}

// process.execPath: el hijo usa exactamente el runtime que lanzó este script.
const child = spawn(process.execPath, [target], { cwd: process.cwd(), stdio: "inherit" });
const [code] = (await once(child, "close")) as [number | null];
process.exit(code ?? 0);
