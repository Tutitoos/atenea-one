<!-- Copyright 2026 Tutitoos -->
<!-- SPDX-License-Identifier: Apache-2.0 -->
# Renovación del certificado de emisor

Cada red tiene un **certificado de emisor**: la raíz offline certifica la clave con la que el control plane firma las configuraciones. Dura como máximo 90 días y, al caducar, el control plane deja de poder emitir configuraciones válidas: los leases de los agentes (300 s más 30 s de tolerancia) expiran y la red se corta. El panel y la aplicación de escritorio avisan desde 14 días antes. Este procedimiento **renueva el certificado conservando la misma clave de emisor**. Rotar la clave de emisor (ADR-005) o la raíz (dos anclas, `--next-anchor`) son otros procedimientos y no están cubiertos aquí.

Los valores `<…>` se sustituyen con el inventario privado del operador. No copiar al repositorio claves, inventarios, direcciones ni resultados reales.

## Reglas que no se pueden saltar

1. **El serial debe ser estrictamente mayor que el vigente.** El servidor **no lo comprueba**: acepta un certificado válido de serial menor. Los agentes, en cambio, rechazan cualquier serial menor que el mayor que han visto (`issuer_rollback`). Subir un serial menor corta la red (ver «Qué pasa si te equivocas»).
2. **Validez de 90 días como máximo.** Según el contrato, el agente rechaza ventanas mayores (no ensayado aquí).
3. **La raíz solo se usa en la estación de administración**, nunca en el VPS ni en un host expuesto (ADR-005).
4. **La clave de emisor descifrada es temporal:** fichero con permisos 0600, sin imprimirla ni pegarla en ningún sitio, y borrada al terminar. Una shell persistente no ejecuta un `trap … EXIT`: la limpieza se hace explícitamente y se comprueba.
5. **Cada red tiene su propio certificado y serial.** Si hay varias redes, se renueva cada una.

## Antes de empezar

- [ ] Fecha: hacerlo **al menos 14 días antes de la caducidad** (cuando el panel empieza a avisar), no el último día.
- [ ] Serial vigente: en el panel, **Auditoría**, último evento `network.issuer_cert`; su detalle trae `serial`. El nuevo será ese número más uno.
- [ ] Caducidad actual: panel, **Red → Certificado de emisor** (o `issuer_cert_not_after` de `GET /api/v1/networks/<uuid-red>`).
- [ ] El control plane está sano y los agentes tienen lease (la renovación no se mezcla con otras incidencias).
- [ ] Reloj de la estación de administración correcto: el certificado empieza a valer en el instante de emisión.
- [ ] Acceso a la raíz offline y a la custodia de secretos (identidad `age`/SOPS) desde la estación de administración.

## Procedimiento

1. **Descifrar la clave de emisor** de la custodia a un fichero temporal privado, sin mostrarla:

   ```sh
   umask 077
   T=$(mktemp -d)
   sops decrypt --extract '["files"]["issuer.key"]' <secretos.sops.yaml> > "$T/issuer.key"
   ```

2. **Emitir el certificado nuevo** con la raíz offline. El identificador de red va sin guiones y el serial es el vigente más uno:

   ```sh
   atenea issuer-cert --root <ruta-raiz.key> --issuer-key "$T/issuer.key" \
     --network-id <uuid-red-sin-guiones> --serial <N+1> --valid-days 90 --out "$T/issuer.cert"
   ```

3. **Subir solo el certificado público.** En el panel: **Red → Certificado de emisor → Reemplazar certificado**, y elegir `issuer.cert`. Por API: `PUT /api/v1/networks/<uuid-red>/issuer-cert` con `Content-Type: application/octet-stream`, sesión y CSRF. Nunca se sube una clave privada.

4. **Borrar la clave temporal y comprobarlo:**

   ```sh
   rm -rf "$T"; ls "$T"   # debe decir que no existe
   ```

## Comprobaciones posteriores

Todas deben cumplirse antes de dar la renovación por buena:

- [ ] El panel (o `issuer_cert_not_after`) muestra la caducidad nueva, unos 90 días desde hoy.
- [ ] **Auditoría** registra un `network.issuer_cert` con `serial` igual al nuevo.
- [ ] En cada equipo, en los 2 minutos siguientes: `lease_active` sigue en `true`, `config.version` avanza (configuración nueva firmada con el serial nuevo) y `sync.last_error` es `none`.
- [ ] La aplicación de escritorio deja de avisar de la caducidad.
- [ ] Apuntar el serial y la fecha de caducidad nuevos para la próxima renovación, y la fecha en que hacerla (caducidad menos 14 días).

## Qué pasa si te equivocas

| Situación | Efecto | Qué hacer |
| --- | --- | --- |
| Serial **menor o igual** que el vigente | Con serial menor, el servidor lo acepta y los agentes responden `issuer_rollback` en `sync.last_error`; dejan de recibir configuración y **pierden el lease** cuando caduca la última válida. Con el mismo serial, es idempotente y no pasa nada. | **Volver a subir de inmediato el certificado de serial mayor.** Los agentes se recuperan solos, sin tocarlos. |
| Certificado de **otra red** o firmado por **otra raíz** | El servidor responde `invalid_certificate` y no cambia nada. | Emitirlo de nuevo con la raíz y el identificador correctos. |
| Certificado de **otra clave de emisor** | El servidor responde `wrong_issuer` y no cambia nada. | Usar la clave de emisor que el control plane tiene instalada. |
| Validez superior a 90 días | Los agentes lo rechazan (`CertExpired`, según el contrato de configuración firmada; **no ensayado**). | Emitirlo con `--valid-days` de 90 o menos. |

No borrar el estado de los agentes ni reemplazar sus anclas para saltarse un rechazo: el rechazo `issuer_rollback` se arregla subiendo el serial correcto.

## Qué se ensayó y con qué límites

Ensayado el 2026-11-02 (`tests/network-lab/issuer-renewal.sh`, que se ejecuta con `bash tests/network-lab/run-e2e.sh issuer-renewal.sh`): control plane real, PostgreSQL, Dex y un agente real en el laboratorio, con **raíz y emisor desechables**. 28 comprobaciones, todas correctas:

- Renovación del serial 1 al 2 y de 30 a 90 días: el servidor informa la caducidad nueva, la auditoría registra `serial: 2`, el **lease no cayó ni una vez** durante 30 s muestreados cada segundo, el agente recibió una configuración firmada con el serial 2 y no hubo errores.
- Subir otra vez el mismo serial es idempotente.
- Rechazos del servidor: certificado de otra red, de otra raíz y de otro emisor (`wrong_issuer`), sin cambiar el certificado vigente.
- **Riesgo medido:** el servidor acepta el serial 1 después del 2; el agente lo informa como `issuer_rollback`, aumentan sus errores de sincronización y se queda sin lease; al volver a subir el serial 2 el agente recupera el lease y el error desaparece solo.

Ensayado también el mismo día **con las claves reales** (raíz offline y clave de emisor de la custodia) sobre una red de prueba desechable en el servidor real, ya borrada: serial 1 de 7 días y renovación a serial 2 de 90 días, aceptados por el servidor, con la caducidad y los eventos de auditoría esperados. Sin agentes en esa red.

Límites, sin comprobar:

- El laboratorio usa validez de configuración de 40 s y un solo agente; los plazos de producción son 300 s más 30 s de tolerancia.
- No se probó un agente apagado durante la renovación (la regla del contrato, serial mayor o igual al persistido, indica que aceptará el nuevo al volver), ni renovar **después** de que el certificado haya caducado.
- No se manejó la pantalla «Reemplazar certificado» del panel (solo el endpoint que usa) ni la aplicación de escritorio.
- El ensayo no incluyó el piloto; la renovación real se hizo después (ver «Renovaciones realizadas»).

## Renovaciones realizadas

| Fecha | Red | Cambio | Resultado |
| --- | --- | --- | --- |
| 2026-11-02 | piloto | serial 1 a 2, 90 días (caduca el 2027-01-31) | Subido por la API, el mismo endpoint que usa el botón del panel (el botón no se manejó). El servidor informó la caducidad nueva y la auditoría registró `serial: 2`. Los tres equipos recibieron configuraciones emitidas tras la subida: el lease no cayó en ningún momento observado (el Mac se muestreó cada unos 25 s durante 3 minutos y recibió tres configuraciones seguidas; el CT y el VPS se comprobaron al cabo de unos 4 minutos), sin `sync.last_error` ni errores de configuración, con 0 errores en el control plane, 9 de 9 servicios sanos y 11 de 11 comprobaciones de tráfico. No se leyó el estado persistido de los agentes (es de root): que aceptan el serial 2 se deduce de que siguen recibiendo y aplicando configuraciones firmadas con el certificado nuevo. |

Próxima renovación del piloto: **antes del 2027-01-17** (14 días antes de la caducidad), con serial **3**.

## Mejora pendiente, no implementada

El control plane podría rechazar con `409` un certificado de serial menor que el instalado, que convertiría el error más grave de este procedimiento en un rechazo inofensivo. Requiere una versión nueva y no debe esperarse para renovar: este runbook funciona sin ella.
