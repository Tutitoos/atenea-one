# Contrato de configuración firmada v3: redes de hasta 500 dispositivos

- **Estado:** Aceptado con [ADR-031](../architecture/ADR-031-conexiones-automaticas-por-aprobacion.md) (2026-11-20). Implementado en `crates/identity`; sin revisión criptográfica externa.
- **Basado en:** [signed-config-v2](signed-config-v2.md) (todo lo no mencionado aquí no cambia) y [signed-config-v1](signed-config-v1.md).
- **Alcance:** solo los contadores y el tamaño del cuerpo `DeviceConfig`. `IssuerCert`, `DeviceBundle`, el envoltorio `SignedConfig` (`magic`, `version` = 1), el dominio de firma y los pasos de verificación no cambian.
- **Número de versión:** el borrador [l2-v1](l2-v1.md) reservaba `version = 3` para L2, que no llegó a implementarse ni a emitirse. Este contrato ocupa el 3; L2 deberá usar la siguiente versión libre cuando se retome.

## 1. Compatibilidad y versión mínima

- El emisor elige siempre la **versión mínima** que representa la configuración: `1` sin campos v2, `2` con ellos, y `3` **solo** cuando hay más de 256 pares o más de 256 reglas. Los bytes de una configuración representable en v1/v2 son idénticos a los anteriores.
- Codificación única: un cuerpo `version = 3` con ≤ 256 pares y ≤ 256 reglas es `Malformed` (debería ser v1/v2). Cambiar el octeto de versión entre 2 y 3 nunca produce un cuerpo válido.
- Un agente anterior devuelve `UnsupportedVersion` ante v3 y **conserva su configuración vigente** (falla cerrado). Por eso el control plane no emite v3 a un dispositivo que no haya declarado la capacidad `config_v3`, y rechaza la operación que lo haría necesario (`409 config_v3_required`) en lugar de recortar pares o permisos.
- Una versión desconocida (≠ 1, 2, 3) es `UnsupportedVersion`.

## 2. Layout y límites

El layout de v3 es **exactamente el de v2** (`Peer` con `routes`, y `advertised`, `dns_servers`, `dns_domains` tras `rules`), aunque esos campos estén vacíos. Solo cambian los máximos:

| Límite | v1/v2 | v3 |
| --- | --- | --- |
| Pares (`peers`, contador u16) | 256 | 499 |
| Reglas (`rules`, contador u16) | 256 | 1536 |
| Cuerpo en bytes | 131 072 | 238 225 |

El resto de contadores (direcciones, endpoints y rutas por par, rutas anunciadas, DNS) es el de v2. 238 225 es el tamaño exacto de un cuerpo con todos los contadores y campos al máximo, de modo que un cuerpo estructuralmente válido nunca supera el límite. El fichero firmado completo ocupa como mucho 251 + 238 225 = 238 476 bytes.

El límite de bytes se decide por el primer octeto del cuerpo antes de interpretar nada más: un cuerpo que declare v1/v2 sigue limitado a 131 072 bytes.

## 3. Reglas estructurales

Las de v1 y v2, sin cambios. Las conexiones automáticas de ADR-031 no añaden campos: el emisor las expresa como reglas explícitas `Device ↔ Device` en ambos sentidos, una por pareja y dirección, que solo autorizan las direcciones exactas del par y nunca sus `routes`.

## 4. Pruebas

`crates/identity/src/v3_tests.rs`: versión mínima por contador con ida y vuelta, no intercambiabilidad v2/v3, límites de bytes por versión en el cuerpo y en el fichero firmado, rechazo de 500 pares y 1537 reglas, e igualdad entre el cuerpo máximo real y el límite declarado. Los vectores v1/v2 existentes siguen aceptándose byte a byte.
