# ADR-006: Licencia del proyecto

- **Estado:** **Aceptado** por el responsable el 2026-10-26. Apache-2.0 se aplicaba desde el primer commit (`LICENSE`, `license = "Apache-2.0"` en el workspace de Cargo); esta decisión la ratifica.
- **Relacionados:** [inventario de dependencias](dependencies.md), [ADR-008](ADR-008-clientes-windows-macos.md) (Wintun), `deny.toml`.

## Contexto

Un repositorio público no concede por sí mismo ningún derecho de uso. El proyecto distribuye código propio (agente, servicios, panel), depende de crates y paquetes de terceros con licencias diversas, y redistribuye de forma controlada un binario con licencia propia (Wintun, solo en Windows).

## Decisión

1. **El código de Atenea One se licencia bajo Apache License 2.0.** Motivos: licencia permisiva y ampliamente conocida, concesión explícita de patentes (relevante en un proyecto de protocolo y criptografía), compatible con las licencias permisivas del ecosistema Rust y con el uso comercial y autoalojado, y sin obligación de publicar modificaciones.
2. **Las dependencias del grafo de producción deben tener licencias compatibles con distribuir el conjunto bajo Apache-2.0.** Lo aplica `cargo deny check licenses` en CI (job `supply-chain`) con la lista cerrada de `deny.toml`: Apache-2.0, MIT, ISC, BSD-2-Clause, BSD-3-Clause, Unicode-3.0, Zlib, CDLA-Permissive-2.0, BSL-1.0 y Unlicense. Añadir una licencia a esa lista es un cambio revisable de esta decisión. No se admiten licencias con copyleft fuerte en el grafo de producción.
3. **Excepciones aceptadas y acotadas:**
   - **Wintun** (DLL oficial de WireGuard LLC, *Prebuilt Binaries License*): solo se usa mediante su API, no se modifica ni se aloja en el repositorio; el instalador la descarga con SHA-256 fijado, verifica la firma y conserva su licencia (ADR-008, `dependencies.md`). No es un crate y no entra en `cargo deny`.
   - **`ring`** llega transitivamente por `snow` con licencia propia (mezcla ISC/OpenSSL), aceptada por el responsable el 2026-10-24; se revisa el texto de la licencia antes del primer release (`dependencies.md`).
   - **`tun` (WTFPL)** no se adoptó: el túnel Linux usa `/dev/net/tun` directamente con `libc`.
4. **Los artefactos distribuidos** (`.deb`, `.pkg`, ZIP de Windows, imágenes) incluyen el texto de la licencia y los avisos de terceros que exijan las licencias redistribuidas. El SBOM de cada release enumera los componentes y sus licencias.
5. **Contribuciones:** se aceptan bajo la misma licencia (Apache-2.0, sección 5), salvo que el contribuyente indique expresamente lo contrario.

6. **Titularidad (declarada por el responsable el 2026-10-26):** `Copyright 2026 Tutitoos`. Se declara en el fichero [`NOTICE`](../../NOTICE) y en dos líneas al principio de cada fichero fuente autorado (`Copyright 2026 Tutitoos` y `SPDX-License-Identifier: Apache-2.0`; tras el shebang si existe). `LICENSE` conserva el texto íntegro de Apache-2.0, sin rellenar el apéndice. No llevan cabecera los ficheros generados, lockfiles, JSON/TOML/YAML/MD/HTML/CSS, fixtures, semillas de fuzz ni las migraciones SQL (sqlx comprueba su checksum). `scripts/add-headers.py` las aplica de forma idempotente y `scripts/check-headers.py`, llamado desde `scripts/check.sh` (y por tanto en CI), falla si falta o difiere; `--fix` la añade a ficheros nuevos.

## Consecuencias

- Quien reciba el software puede usarlo, modificarlo y redistribuirlo, incluso comercialmente, conservando los avisos y la licencia; la concesión de patentes se retira para quien litigue por patentes contra el proyecto.
- La política de licencias de dependencias es automática y verificable en cada cambio.
- Redistribuir Wintun junto a Atenea está limitado por su licencia propia; sustituir el controlador exigiría revisar ADR-008.

## Pendiente

- Revisión del texto de la licencia de `ring` antes del primer release.
- Las licencias de las dependencias transitivas del frontend (pnpm) no se escanean con la misma lista cerrada que las de Rust.
