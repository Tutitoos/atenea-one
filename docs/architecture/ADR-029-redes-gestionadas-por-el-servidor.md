<!-- Copyright 2026 Tutitoos -->
<!-- SPDX-License-Identifier: Apache-2.0 -->
# ADR-029: Redes gestionadas por el servidor (crear una red con solo un nombre)

- **Estado:** **Aceptado** por el responsable el 2026-11-18, con las seis decisiones D1 a D6 tal como se propusieron. **Implementado en `main`, sin publicar ni desplegar** (control plane: migración `0017`, `managed.rs`, renovación automática, CIDR automáticos; panel: crear solo con nombre, «Añadir equipo», «Permitir todo entre equipos»). Guía: [cuentas y redes compartidas](../operations/cuentas-y-redes-compartidas.md). **Pendiente:** rotación de la clave de envoltura, prueba con un equipo real, y simulacro de restauración con la clave.
- **Relacionados:** [ADR-005](ADR-005-custodia-clave-raiz.md) (modifica su regla «raíz en el host del control plane: prohibido» solo para las redes gestionadas), [ADR-028](ADR-028-cuentas-propias-y-redes-por-cuenta.md) (D1: raíz bajo custodia del usuario, que pasa a ser el modo avanzado), [ADR-018](ADR-018-union-solo-con-el-codigo.md) (el coordinator lee las anclas de las redes por API), [ADR-002](ADR-002-revocacion-lease.md).

## Contexto

Hechos comprobados con el panel de la 0.1.34 más ADR-028:

- Crear una red pide un nombre, dos CIDR y el **ancla pública de una raíz que el usuario genera con `atenea root-init`**, y después firmar y subir a mano el certificado de emisor. Son cinco pasos manuales antes de poder unir un equipo.
- Cuando el usuario perdía la raíz, la red dejaba de funcionar al caducar el certificado (ADR-005): era el coste de que el servidor no pudiera tocarla.
- El responsable lo descarta para el uso normal: debe ser **automático, como ZeroTier y Tailscale**. Se crea la red, se copia un comando del CLI en el equipo y el equipo queda unido, sin que el usuario vea claves.
- La unión de equipos ya es casi así: el panel genera un código con token y `atenea network join '<código>' --up` une el equipo (ADR-018). Lo manual es solo crear la red.

Lo que implica: para que nadie firme nada a mano, el servidor tiene que poder firmar el certificado de emisor de cada red, es decir, **tiene que poseer la clave raíz** de las redes que crea por sí mismo. ZeroTier y Tailscale funcionan así: quien controla el servidor controla todas las redes.

## Decisión propuesta

### D1. Dos modos de custodia por red

- **Gestionada (por defecto):** el servidor genera la raíz, firma y renueva solo el certificado de emisor. El usuario no ve ninguna clave.
- **Propia (avanzado, ya implementado en ADR-028):** el usuario aporta el ancla y firma él. Se mantiene tal cual para quien quiera la garantía de que el servidor no puede emitir por su red.

El modo se fija al crear la red y **no cambia** (el modo ya es inmutable por red en otros aspectos, p. ej. L2). Las cinco redes del piloto son «propia» y no se tocan.

### D2. Crear una red solo pide el nombre

`POST /api/v1/orgs/{org}/networks {name}` crea una red gestionada. Los CIDR se asignan solos desde un bloque privado reservado (por defecto `10.64.0.0/10`, IPv4 /24 por red, e IPv6 ULA /64 derivado del identificador de la red), sin solapes dentro de la organización. Quien quiera rangos concretos los puede indicar, y con ancla indica el modo «propia».

### D3. La raíz gestionada vive cifrada, no en claro

- Se genera con el mismo Ed25519 y el mismo formato de certificado de ADR-005 (**no se inventa ninguna primitiva** ni se cambia ningún formato firmado ni el agente).
- En la base se guarda **cifrada con una clave de envoltura** que vive en un fichero 0600 distinto del de la base y fuera de sus copias (`ATENEA_CP_ROOT_WRAP_KEY_FILE`), como la clave de emisor. Un AEAD estándar de RustCrypto; el algoritmo exacto se fija en la implementación y se documenta en `dependencies.md`.
- Se descifra **solo en memoria** durante el instante de firmar un certificado y se borra después (`zeroize`).
- Sin esa clave de envoltura configurada, el servidor **no crea redes gestionadas** (falla cerrado) y las redes propias siguen funcionando.

### D4. Certificado de emisor automático

Una tarea del control plane firma el certificado de emisor de cada red gestionada al crearla (30 días) y lo renueva con un `issuer_serial` mayor cuando le queda un tercio de la vida. Los agentes lo aceptan igual que hoy (serial monótono, ADR-005). Un fallo de renovación se registra y se avisa en el panel; no se borra nada.

### D5. Unir un equipo es un comando

Tras crear la red, el panel muestra «Añadir equipo» con un único comando listo para copiar, `sudo atenea network join '<código con token>' --up`, con el token de un solo uso ya generado (1 hora, aprobación automática para el propietario). El equipo genera sus claves en local, como siempre, y queda conectado. Aprobar a mano y las políticas siguen siendo deny por defecto (ADR-016): **unir un equipo no le da acceso a los demás hasta que haya una política**; el panel ofrece crear en un clic la política «todos con todos» para el caso de la red personal.

### D6. Lo que se acepta y se dice de frente

- **Quien comprometa el servidor puede emitir por cualquier red gestionada.** Es lo mismo que en ZeroTier y Tailscale y es el coste de lo automático. Mitigaciones: raíz cifrada en reposo con clave aparte, descifrado solo al firmar, certificados cortos, auditoría de cada firma (`network.issuer_cert` con `actor_kind = system`), y el modo «propia» para quien no lo acepte.
- Una copia de la base **sin** la clave de envoltura no revela las raíces. Perder la clave de envoltura equivale a perder todas las raíces gestionadas: hay que guardarla con el mismo cuidado que la clave de emisor y probarlo en el simulacro de restauración.
- Esto **debilita deliberadamente ADR-005 para las redes gestionadas** y exige que el modelo de amenazas lo recoja antes de publicar.

## Alternativas descartadas

| Opción | Motivo |
| --- | --- |
| Solo modo gestionado | Quita la garantía de custodia a quien la necesita y contradice ADR-028 D1; el modo propio ya existe y cuesta poco mantenerlo. |
| Raíz generada en el navegador | No evita el firmado manual ni la pérdida de la clave; el usuario sigue gestionando claves. |
| Raíz en HSM o KMS externo | Mejor seguridad, pero añade una dependencia de operación que el piloto no tiene; se reevalúa tras la auditoría. |
| Raíz en claro en la base | Una copia de la base entregaría todas las redes. |

## Fases (tras aceptar)

1. Migración (`networks.root_mode`, raíz cifrada), clave de envoltura y firma automática en el control plane, con pruebas contra PostgreSQL 17.
2. Panel: crear red solo con nombre, «Añadir equipo» con el comando y la política en un clic; el flujo con raíz propia pasa a «Avanzado».
3. Modelo de amenazas, guía de operación, simulacro de restauración con la clave de envoltura y prueba con un equipo real en un servidor local.
4. Una versión solo si el responsable la pide.

## Sin comprobar

Comprobado con PostgreSQL 17 y un navegador real contra un servidor local: crear solo con nombre, certificado firmado y validado, renovación, comando de unión y política en un clic. **No comprobado:** unir un equipo real con ese comando, el simulacro de restauración con la clave de envoltura y la rotación de esa clave. Sin comprobar también: el bloque de direcciones por defecto frente a las redes del piloto, y cómo se comporta la renovación con el servidor parado varios días.
