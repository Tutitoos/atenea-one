import { URL } from 'node:url';
import { readFile, writeFile } from 'node:fs/promises';
import { createHash } from 'node:crypto';
const file = new URL('../build/client/index.html', import.meta.url);
let html = await readFile(file, 'utf8');
const scripts = [...html.matchAll(/<script\b([^>]*)>([\s\S]*?)<\/script>/g)];
for (const [tag, attrs, body] of scripts) {
  if (/\bsrc\s*=/.test(attrs) || !body.trim()) continue;
  const name = `boot-${createHash('sha256').update(body).digest('hex').slice(0, 16)}.js`;
  await writeFile(new URL(`../build/client/assets/${name}`, import.meta.url), body);
  html = html.replace(tag, `<script${attrs} src="/assets/${name}"></script>`);
}
if (/<script\b(?![^>]*\bsrc=)[^>]*>\s*\S/.test(html)) throw new Error('El panel contiene scripts inline.');
await writeFile(file, html);
