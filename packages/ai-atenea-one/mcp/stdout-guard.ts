// Copyright 2026 Tutitoos
// SPDX-License-Identifier: Apache-2.0
import { createWriteStream } from "node:fs";

// El transporte MCP stdio usa stdout para tramas JSON-RPC: cualquier otra
// escritura (un console.log, un aviso de dependencia) desincroniza al cliente.
// Este módulo solo importa `node:fs`, así que se evalúa antes que el resto de
// imports de `server.ts` y redirige `process.stdout.write` a stderr.
const stderrWrite = process.stderr.write.bind(process.stderr);

process.stdout.write = ((
  chunk: string | Uint8Array,
  encodingOrCallback?: BufferEncoding | ((err?: Error | null) => void),
  callback?: (err?: Error | null) => void,
): boolean => {
  if (typeof encodingOrCallback === "function") return stderrWrite(chunk, encodingOrCallback);
  if (encodingOrCallback === undefined && callback === undefined) return stderrWrite(chunk);
  return stderrWrite(chunk, encodingOrCallback as BufferEncoding, callback);
}) as typeof process.stdout.write;

/** Escritor sobre el fd 1 que esquiva la redirección anterior: solo para el transporte MCP. */
export const sanctionedStdout = createWriteStream("", { fd: 1 });
