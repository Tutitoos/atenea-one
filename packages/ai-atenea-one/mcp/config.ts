// Copyright 2026 Tutitoos
// SPDX-License-Identifier: Apache-2.0
import { readFileSync, statSync } from "node:fs";
import { join } from "node:path";

export type Config = { url: string; token: string };

const TOKEN_RE = /^atsvc_[A-Za-z0-9_-]{43}$/;

/** Valores `clave = "valor"` del fichero; el TOML de este fichero no admite nada más. */
function parseToml(text: string): Record<string, string> {
  const out: Record<string, string> = {};
  for (const m of text.matchAll(/^\s*(\w+)\s*=\s*"([^"\n]*)"\s*(?:#.*)?$/gm)) out[m[1]] = m[2];
  return out;
}

/** Ninguno de los errores incluye el valor del token. */
export function loadConfig(env: Record<string, string | undefined>, homeDir: string): Config {
  let url = env.ATENEA_AI_URL;
  let token = env.ATENEA_AI_TOKEN;
  if (!url || !token) {
    const file = join(homeDir, ".config", "ai-atenea-one", "config.toml");
    let st;
    try {
      st = statSync(file);
    } catch {
      throw new Error(`Falta configuración: define ATENEA_AI_URL y ATENEA_AI_TOKEN o crea ${file} (modo 0600) con url y token.`);
    }
    if ((st.mode & 0o077) !== 0) throw new Error(`${file} debe tener modo 0600 (chmod 600): contiene el token.`);
    const cfg = parseToml(readFileSync(file, "utf8"));
    url = cfg.url;
    token = cfg.token;
  }
  if (!url || !/^https?:\/\/[^\s/]+/.test(url)) throw new Error("La URL del control plane debe ser http(s)://host[:puerto].");
  if (!token || !TOKEN_RE.test(token)) throw new Error("El token de servicio debe tener la forma atsvc_<43 caracteres base64url>.");
  return { url: url.replace(/\/+$/, ""), token };
}
