#!/usr/bin/env python3
# Copyright 2026 Tutitoos
# SPDX-License-Identifier: Apache-2.0
"""Comprueba enlaces locales e higiene del paquete; no modifica archivos ni usa red."""
from pathlib import Path
import re
import sys
from urllib.parse import unquote, urlsplit

ROOT = Path(__file__).resolve().parents[2]
LINK = re.compile(r'!?\[[^\]\n]*\]\(([^\s)]+)(?:\s+"[^"\n]*")?\)')
REFERENCE = re.compile(r'^\s*\[[^\]]+\]:\s*(\S+)', re.MULTILINE)
PATTERNS = {
    'nombre privado prohibido': re.compile('ke' + 'na', re.IGNORECASE),
    'ruta personal absoluta': re.compile(r'/Users/|/home/[^/\s]+/|[A-Z]:\\Users\\', re.IGNORECASE),
    'clave privada PEM': re.compile(r'-----BEGIN (?:[A-Z0-9]+ )*PRIVATE KEY-----'),
    'credencial GitHub': re.compile(r'\b(?:gh[pousr]_[A-Za-z0-9]{20,}|github_pat_[A-Za-z0-9_]{40,})\b'),
    'credencial AWS': re.compile(r'\b(?:AKIA|ASIA)[A-Z0-9]{16}\b'),
    'JWT literal': re.compile(r'\beyJ[A-Za-z0-9_-]{12,}\.[A-Za-z0-9_-]{12,}\.[A-Za-z0-9_-]{12,}\b'),
    'credencial en URL': re.compile(r'https?://[^\s/@:]+:[^\s/@]+@'),
    'asignación de secreto literal': re.compile(
        r'''(?i)\b(?:password|secret|api_key|access_token|private_key)\s*[:=]\s*["'](?!<|\$)[A-Za-z0-9_+/=-]{20,}["']'''),
}


def headings(text):
    slugs = set()
    counts = {}
    in_fence = False
    for line in text.splitlines():
        if line.lstrip().startswith(('```', '~~~')):
            in_fence = not in_fence
        if in_fence:
            continue
        match = re.match(r'^#{1,6}\s+(.+?)\s*#*$', line)
        if not match:
            continue
        title = re.sub(r'\[([^\]]+)\]\([^)]*\)', r'\1', match[1])
        title = re.sub(r'<[^>]*>', '', title).lower()
        slug = re.sub(r'[^\w\s-]', '', title)
        slug = re.sub(r'\s', '-', slug)
        count = counts.get(slug, 0)
        counts[slug] = count + 1
        slugs.add(slug + ('-' + str(count) if count else ''))
    slugs.update(re.findall(r'\bid=["\']([^"\']+)', text))
    return slugs


def main():
    # Small deterministic parser self-check; no files are written.
    assert headings('# Título: `MAC1`\n# Título: `MAC1`\n') == {'título-mac1', 'título-mac1-1'}
    assert LINK.findall('[ejemplo](../protocol/wire-v2.md#2-primitivas-y-bytes-exactos)') == [
        '../protocol/wire-v2.md#2-primitivas-y-bytes-exactos']
    files = sorted((ROOT / 'docs' / 'audit').glob('*.md'))
    if not files:
        raise SystemExit('No hay documentos de auditoría')
    errors = []
    links = 0
    for file in files:
        text = file.read_text()
        relative = file.relative_to(ROOT)
        for description, pattern in PATTERNS.items():
            for match in pattern.finditer(text):
                line = text.count('\n', 0, match.start()) + 1
                errors.append(f'{relative}:{line}: {description}')
        for raw in LINK.findall(text) + REFERENCE.findall(text):
            url = urlsplit(raw.strip('<>'))
            if url.scheme or url.netloc:
                continue
            links += 1
            target = (file.parent / unquote(url.path)).resolve() if url.path else file
            if not target.is_relative_to(ROOT) or not target.exists():
                errors.append(f'{relative}: destino inexistente o externo: {raw}')
            elif url.fragment and target.suffix == '.md':
                if unquote(url.fragment) not in headings(target.read_text()):
                    errors.append(f'{relative}: fragmento inexistente: {raw}')
    if errors:
        print('\n'.join(errors), file=sys.stderr)
        return 1
    print(f'OK: {len(files)} documentos, {links} enlaces relativos (destinos y fragmentos), '
          f'{len(PATTERNS)} patrones de higiene; sin incidencias.')
    print('Alcance: docs/audit/*.md; no prueba ausencia absoluta de secretos ni revisa historia Git.')
    return 0


if __name__ == '__main__':
    sys.exit(main())
