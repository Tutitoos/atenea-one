# ADR-005 — Custodia de la clave raíz de firma y emisores delegados

- **Estado:** Propuesto (recomendación; pendiente de aceptación)
- **Fecha:** 2026-10-24
- **Debe resolverse antes de:** fase 3
- **Relacionados:** ADR-002

## Contexto

Los agentes confían en configuraciones firmadas (ADR-002). ADR-002 exige reemitirlas cada 60 s, así que **hay una clave de firma disponible online**. Si fuera la misma que ancla la confianza y viviera en el host expuesto (control plane), comprometer ese host entregaría la raíz de confianza. E2E no resuelve ese compromiso: una autoridad comprometida puede autorizar identidades y configuraciones maliciosas.

## Decisión

**Dos niveles de clave, Ed25519.**

1. **Clave raíz.** Se genera en la estación de administración, **nunca en un host expuesto**, y se usa solo para firmar certificados de emisor. Su clave pública es el **trust anchor** que cada agente guarda al incorporarse (pinning).
2. **Clave de emisor** (online). Firma las configuraciones por dispositivo. Su certificado, firmado por la raíz, contiene: clave pública del emisor, `issuer_serial` (monótono), ámbito (`network_id`) y validez.

Parámetros propuestos y reglas:

| Elemento | Decisión |
| --- | --- |
| Algoritmo | Ed25519; verificación estricta (`ed25519-dalek 3.0.0`, API a confirmar al fijar) |
| Separación de dominios | Cada firma lleva un prefijo fijo por tipo (`atenea-one/issuer-cert/v1`, `atenea-one/config/v1`) |
| Codificación firmada | Binaria, determinista y versionada (la define el contrato de la fase 0; no JSON canónico) |
| Validez del certificado de emisor | 30 días en laboratorio; se acorta en el piloto |
| Almacenamiento de la raíz (MVP-0) | Fichero cifrado con contraseña (`age`, scrypt) fuera del repositorio, con dos copias cifradas fuera de línea |
| Almacenamiento de la raíz (antes del piloto) | Token hardware con Ed25519 (p. ej. PIV/OpenPGP); decisión de hardware pendiente del usuario |
| Almacenamiento del emisor | Fichero 0600 en el host emisor, usuario de servicio no root; en el laboratorio, la estación de administración; en 3B, el control plane |
| Raíz en el host del control plane | **Prohibido** |

### Compromiso del emisor

- Un emisor comprometido puede autorizar dispositivos y redes hasta que caduque su certificado.
- **Recuperación:** la raíz emite un certificado nuevo con `issuer_serial` mayor. Los agentes persisten el mayor serial visto y rechazan menores (ADR-002). Un dispositivo que aún no ha visto el nuevo serial sigue confiando en el emisor viejo hasta su caducidad: **el radio de daño está acotado por la validez del certificado**, por eso se acorta en el piloto.

### Rotación y pérdida de la raíz

- El agente admite hasta **dos trust anchors** (raíz actual y siguiente) para rotar sin cortar el servicio: la raíz nueva se añade en una configuración firmada por la actual.
- **Pérdida o compromiso de la raíz:** no hay recuperación automática. Hay que reincorporar manualmente cada dispositivo con un trust anchor nuevo. Se documenta como procedimiento en `docs/operations/` y se ensaya en la fase 9.

### Qué protege el pinning y qué no

- Protege frente a: un emisor falso, un control plane sustituido o un atacante en la red que sirva configuraciones.
- No protege frente a: compromiso de la raíz, ni de un emisor mientras su certificado sea válido, ni de un dispositivo que haya sido incorporado con un trust anchor equivocado.

## Alternativas

| Opción | Motivo |
| --- | --- |
| Clave única en el emisor | Un host expuesto con la raíz: se descarta. |
| Raíz offline que firma cada configuración | Incompatible con reemisión cada 60 s. |
| HSM o token desde el primer día | Mejor seguridad; añade dependencia de hardware en MVP-0. Se exige antes del piloto, no del laboratorio. |
| Umbral/multifirma (varios firmantes) | Fuera de alcance de v1; se reevalúa tras la auditoría. |

## Consecuencias

- El agente implementa verificación de dos niveles (cadena de dos eslabones), más estado persistente de serial y versión.
- La raíz tiene un ciclo de uso manual y poco frecuente; renovar emisores cada 30 días es una tarea operativa.
- Las claves de dispositivo (Ed25519 de identidad y X25519 de handshake) se generan en el dispositivo y nunca salen; el emisor solo recibe claves públicas.

## Verificación (fase 3)

Configuración firmada por emisor cuyo certificado caducó o con serial menor rechazada; emisor firmado por una raíz distinta rechazado; cambio de ámbito (`network_id`) rechazado; dominio de firma cruzado (firma de configuración usada como certificado) rechazado; rotación a una segunda raíz sin cortar el túnel; verificación de que la clave raíz no existe en ninguna ruta de despliegue.

## Pendiente del usuario

Elegir el token hardware y confirmar los 30 días de validez de laboratorio.
