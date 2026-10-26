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

## Consecuencias

- Quien reciba el software puede usarlo, modificarlo y redistribuirlo, incluso comercialmente, conservando los avisos y la licencia; la concesión de patentes se retira para quien litigue por patentes contra el proyecto.
- La política de licencias de dependencias es automática y verificable en cada cambio.
- Redistribuir Wintun junto a Atenea está limitado por su licencia propia; sustituir el controlador exigiría revisar ADR-008.

## Pendiente

- Titularidad del copyright: el texto de `LICENSE` es el de Apache-2.0 sin rellenar los campos de ejemplo del apéndice. Si el responsable quiere declarar un titular y año (cabeceras o un fichero `NOTICE`), debe indicarlo; no se inventa.
- Revisión del texto de la licencia de `ring` antes del primer release.
- Las licencias de las dependencias transitivas del frontend (pnpm) no se escanean con la misma lista cerrada que las de Rust.
