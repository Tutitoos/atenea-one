<!-- Copyright 2026 Tutitoos -->
<!-- SPDX-License-Identifier: Apache-2.0 -->
# ADR-033: Arranque de los servicios de red de Windows por llegada de la primera IP

- **Estado:** **Aceptado** por el responsable el 2026-11-20 («empieza por el 2 y hazlo con su ADR, que quede permanente en el CLI»). **Implementado en una rama, sin publicar ni desplegar.**
- **Relacionados:** [ADR-017](ADR-017-redes-por-instancia-en-windows.md) (§1 y decisión 2: servicio por red en arranque automático retrasado; sin ejecutar `sc.exe`), [ADR-032](ADR-032-conexion-con-los-pares-al-arrancar.md) (cuyas mediciones usaron este arranque configurado a mano), [ADR-030](ADR-030-cambio-de-red-y-despertar.md).

## Contexto

[ADR-017](ADR-017-redes-por-instancia-en-windows.md) dejó el servicio de cada red (`AteneaNetwork-<id8>`) en **arranque automático retrasado** para que la red vuelva tras reiniciar, como en Linux y macOS. Medido el 2026-11-20 en un Windows real (cable, agente 0.1.35):

- Con el modo retrasado, tras un reinicio el servicio arrancó a los **134 s** del arranque del kernel (una medida; no se repitió con ese modo).
- Con el servicio en inicio manual y un disparador del SCM «llega la primera dirección IP» configurado a mano con `sc triggerinfo`, arrancó a los **12–13 s** del arranque del kernel en los 14 reinicios medidos (tres builds distintas del agente). En los que se leyó el registro, su primera configuración quedó aplicada a los ~28 s.
- La red de Windows (Ethernet por DHCP) estaba lista a los 13 s en la medida que se tomó.

Ese ajuste manual no sobrevive: `atenea network up` vuelve a fijar el arranque retrasado. Y la política de [ADR-017 §1](ADR-017-redes-por-instancia-en-windows.md) es no ejecutar `sc.exe` desde la CLI: se usa la API del SCM.

## Decisión

### D1. Arranque manual con disparador de primera IP

El servicio de cada red queda en **inicio manual** con un único disparador del SCM, solo de arranque: `SERVICE_TRIGGER_TYPE_IP_ADDRESS_AVAILABILITY` con el subtipo `NETWORK_MANAGER_FIRST_IP_ADDRESS_ARRIVAL_GUID` (`4f27f2de-14e2-430b-a549-7cd48cbc8245`). Windows lo arranca cuando llega la primera dirección IP tras el arranque del equipo, en vez de esperar al retraso de los servicios automáticos.

### D2. `up`, `down` y servicios existentes

- `up` registra el servicio (si no existe) en inicio manual y le fija el disparador; sigue iniciándolo en el momento y manteniendo el reinicio tras fallo (5, 15 y 60 s, también ante salidas con error). Es idempotente.
- Un servicio creado por una versión anterior (automático retrasado) se migra en el siguiente `up`: se quita la marca de arranque retrasado mientras aún es automático y después se pasa a manual con el disparador.
- `down` no cambia: para el servicio y lo deja **deshabilitado**; un servicio deshabilitado no arranca ni por disparador. `up` lo vuelve a habilitar. El desinstalador (que deshabilita los servicios con `sc config … start= disabled`) tampoco cambia.

### D3. Implementación

`windows-service` no expone disparadores. Se añade un módulo mínimo `crates/windows/src/trigger.rs` con una sola llamada FFI a `ChangeServiceConfig2W(SERVICE_CONFIG_TRIGGER_INFO)` sobre el manejador del servicio, con el mismo tratamiento que los otros módulos con `unsafe` del crate (`#[allow(unsafe_code)]` local y comentario `SAFETY`). Usa `windows-sys`, que ya es dependencia: solo se activa su característica `Win32_System_Services`. No se añade ningún crate ni se ejecuta `sc.exe`.

### Lo que no cambia

Protocolo, formatos firmados, agente, instalador y datos. El servicio sigue siendo LocalSystem con el mismo binario y línea de órdenes.

## Consecuencias y límites

- El disparador actúa **una vez por arranque del equipo**. Un `Stop-Service` a mano deja el servicio parado hasta el siguiente arranque (igual que con el arranque automático); una caída la recupera el reinicio tras fallo.
- Si el equipo arranca sin red, el servicio arranca cuando aparece la primera IP, no antes.
- La «primera dirección IP» puede ser la de otro adaptador (virtual, por ejemplo) y no la de la red física. **No se ha comprobado** cómo se comporta el agente si arranca sin ruta hacia el servidor; la configuración en caché y el sondeo de cambio de red (ADR-030) deberían cubrirlo, pero no hay una medición.
- Pasa de un comportamiento documentado de Windows («retrasado») a otro («disparador»); si una versión futura de Windows cambiara el GUID o su semántica, el servicio quedaría en manual sin arrancar solo. La prueba de aceptación de la CI comprueba que el disparador está registrado, no que arranque tras un reinicio.

## Alternativas descartadas

- **Mantener el arranque retrasado:** simple y documentado, pero medido en 134 s de espera tras el arranque.
- **Arranque automático sin retraso:** arrancaría antes de que haya red y sin la garantía del disparador; no se midió.
- **Dependencia del servicio de ubicación de red (`NlaSvc`):** garantiza que el servicio de estado de red existe, no que haya una IP.
- **`sc.exe triggerinfo` desde la CLI:** vale para la prueba manual, pero ADR-017 §1 prohíbe ejecutar `sc.exe` desde la CLI.

## Verificación

- La prueba de aceptación de Windows (`tests/windows/network-checks.ps1`, en la CI) pasa a comprobar tras `up`: inicio manual, disparador `4f27f2de-…` presente y sin marca de arranque retrasado, y la migración desde un servicio automático retrasado.
- **Comprobado el 2026-11-20 en el Windows de pruebas** con el instalador de la rama (commit `5f48a7c`, compilado por el workflow `release` sin publicar; sin firma): se devolvió el servicio al estado antiguo (automático retrasado, sin disparador), `atenea network up` lo migró a inicio manual con el disparador de primera IP (sin marca de retraso y con la recuperación 5/15/60 s intacta) y, en dos reinicios con la aplicación arrancando sola, el ping Mac→Windows por la VPN respondió a los **29 s y 29 s** del arranque del kernel (36–37 s con el servicio ya configurado a mano en ADR-032; hasta 152 s en el modo retrasado). La medida es de ping, no de arranque del servicio; la diferencia con ADR-032 es de una máquina y una red, no se atribuye a este cambio.
- **No comprobado:** el comportamiento del agente al arrancar sin ruta hacia el servidor (ver Consecuencias).
