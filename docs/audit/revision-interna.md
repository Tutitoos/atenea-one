# Revisión interna de seguridad (fase 11): resumen sanitizado para el auditor externo

Referencia de código de la revisión histórica: `a0f8ac71b5e83607603c45b46113a2aa0ea607b6` (2026-10-26); las correcciones y mediciones posteriores se distinguen en §6.3 y no se atribuyen a ese commit. Este documento **no es una auditoría ni una garantía**: resume una revisión interna hecha por lectura, lo que se cambió después y lo que sigue abierto. La auditoría independiente sigue pendiente y es bloqueante ([matriz, caso «Auditoría independiente»](../operations/matriz-de-aceptacion.md)).

## Etiquetas de evidencia

- **VERIFICADO**: lo he comprobado leyendo el fichero enlazado en esta redacción (no ejecutando nada).
- **PARCIAL**: hay evidencia, pero falta parte del criterio.
- **SIN COMPROBAR**: no hay evidencia en el repositorio ni la he leído.
- **INFERIDO**: deducción mía a partir de lo leído, no observada.
- No se ejecutaron pruebas, builds ni CI para esta actualización documental; se citan resultados declarados por otros documentos o comunicados del laboratorio, distinguiendo su procedencia.

## 1. Qué fue la revisión y qué límites tiene

| Frente | Alcance (lectura) | Método |
| --- | --- | --- |
| A. Criptografía, protocolo, identidad | `crates/{crypto,protocol,identity,nat-traversal}`, motor/estado/sincronización del agente, ADR-001/002/003/005, contrato de configuración firmada, fuzzing | Solo lectura, sin ejecutar nada |
| B. Agente local, privilegios, plataformas, paquetes, cadena de suministro | IPC, ficheros privados, nftables/rutas/DNS, TUN, empaquetado Linux/macOS/Windows, CI y release | Solo lectura, sin ejecutar nada |
| C. Servicios y autorización | control plane, coordinator, relay, panel, despliegue Compose/Ansible | Solo lectura, sin ejecutar nada |

- **VERIFICADO (lectura de los tres informes de trabajo de la revisión)**: los informes originales **no están en el repositorio** (son artefactos de sesión). Este resumen es su única huella versionada; el auditor no puede contrastar línea a línea. Los números de línea que citaban eran los vistos al revisar el árbol anterior a las correcciones (ver §4) y hoy no son fiables.
- Los informes declaran expresamente: sin ejecución dinámica; los hallazgos de denegación de servicio del frente C se apoyan en lectura y **no** en tasas medidas; no se revisó soundness de dependencias externas (`ring`, `snow`, `rustls`) ni la configuración real del environment `release` de GitHub ni hay base RustSec offline.
- Los informes **no** cubren el código posterior (wire v2, relay v2, cookies por par, parada acotada). Esas piezas se revisaron solo por pruebas/documentos propios (§5), no por una segunda revisión de lectura independiente: **SIN COMPROBAR** como revisión.

## 2. Recuento por severidad y frente

Hallazgos de los informes históricos (árbol previo a las correcciones). Los identificadores `F#` se repiten entre frentes; aquí se citan como A-F#, B-F#, C-F#.

| Frente | Crítica | Alta | Media | Baja | Informativa | Total |
| --- | ---: | ---: | ---: | ---: | ---: | ---: |
| A. Cripto/protocolo/identidad | 0 | 0 | 3 (A-F1–F3) | 5 (A-F4–F8) | 5 (A-F9–F13) | 13 |
| B. Agente/plataformas/suministro | 0 | 0 | 5 (B-F1–F5) | 9 (B-F6–F14) | 5 (B-F15–F19) | 19 |
| C. Servicios/autorización | 0 | 1 (C-F1) | 6 (C-F2–F7) | 6 (C-F8–F14, ver nota) | 4 (I1–I4) | 17 |
| **Total** | **0** | **1** | **14** | **20** | **14** | **49** |

Notas de recuento (VERIFICADO en los informes):
- A-F5 figura como «Baja/Media»; se cuenta como Baja, como hace su tabla.
- C declara 6 Bajas pero enumera C-F8–F14 (siete entradas; C-F14 agrupa ruido de higiene). Se respeta la cifra de su cabecera.
- Todos los hallazgos están etiquetados «CONFIRMADO» (por lectura), «SOSPECHA» o «NO COMPROBADO» en el informe original; nada fue reproducido dinámicamente.

## 3. Temas de los hallazgos (sin detalle explotable)

| Tema | Fuente | Naturaleza |
| --- | --- | --- |
| Limitadores de abuso globales o agotables antes de autenticar (motor, relay, coordinator, enrolamiento, login, endpoints firmados) | A-F1, C-F1 (única Alta), C-F2–C-F5, C-F10, C-F11, B-F2 | Disponibilidad |
| Estado anti-rollback y reloj: avance con hora cruda; persistencia que podía retroceder; sincronización que sustituía la configuración antes de verificar | A-F2, A-F3, A-F5 | Disponibilidad / integridad del estado |
| Handshake pendiente descartado por mensajes sin autenticar; frescura del mensaje 1 solo en memoria; roaming sin validación de retorno | A-F4, A-F6, A-F7 | Disponibilidad; límite de diseño |
| Higiene de secretos y ficheros privados (`zeroize`, propietario, enlaces simbólicos, temporales) | A-F8, B-F8, B-F12, B-F19 | Local multiusuario |
| IPC y bucle del plano de datos; DNS residual tras muerte del proceso en macOS/Windows; rutas del sistema por `PATH`; endurecimiento de unidad | B-F1, B-F3, B-F9–B-F11, B-F14 | Privilegios locales / disponibilidad |
| Cadena de suministro: acciones sin fijar por SHA, environment no demostrable, sin escaneo de dependencias ni toolchain fijado | B-F4, B-F5 | Suministro |
| Autorización: la regla inversa de la ACL sin puertos; CIDR de red no restringido; endpoints administrados sin sanear | C-F6, C-F7, C-F12 | Autorización / mínimo privilegio |
| MFA no demostrada en la pila de referencia; retención y cuotas | C-F9, C-F13 | Operación |
| Documentación desfasada (tamaños de mensaje, README de fuzz) y cobertura de fuzz insuficiente | A-F13 y apartado «Pruebas y fuzzing» de A | Calidad de evidencia |

Conclusión negativa declarada por los informes (no es garantía): no se encontró escalada local a root por IPC, ni suplantación de origen entre pares en la TUN, ni fallos de corrección en Noise/nonce/antirreplay/dominios de firma/parseo, ni evasión del plano de control por nftables/rutas, ni IDOR entre organizaciones/redes en el control plane.

## 4. Correcciones: qué consta en el repositorio

Estado histórico del código corregido = commits siguientes. **INFERIDO** que corrijan el hallazgo: lo he emparejado por nombre de prueba y mensaje de commit, **no** he ejecutado nada ni releído cada cambio línea a línea. Las correcciones posteriores del índice de `ip_allocations`, renovación anticipada del relay y reintento de presencia se separan en §6.3.

| Commit | Contenido (mensaje de git) |
| --- | --- |
| [`53351b6`](https://github.com/Tutitoos/atenea-one/commit/53351b6e0b63ce02b3eabae51e5c5b2e7f1e2b79) (2026-10-25) | «correcciones de la revisión de seguridad interna (motor, estado, servicios, relay, IPC, plataformas, cadena de suministro, fuzzing)»: 111 ficheros |
| [`151d3d7`](https://github.com/Tutitoos/atenea-one/commit/151d3d7627d55c69d7db0bbedc17b63f59a51d7d) | `cargo-deny` operativo, `publish=false`, `rustls-pemfile` sustituido, excepciones temporales justificadas |
| [`954a503`](https://github.com/Tutitoos/atenea-one/commit/954a50393b5f56061788052902e896593e601144) | Fuzzing sin red; registro de la primera ejecución de los trece objetivos (13 ficheros en [`fuzz/fuzz_targets/`](../../fuzz/fuzz_targets/), VERIFICADO por listado) |
| [`d9ab7ec`](https://github.com/Tutitoos/atenea-one/commit/d9ab7ec4b4af44dd022ab655b7747c16231a6f8b) | wire v2 (MAC1/MAC2/cookie, versión de cable), relay v2 (`relay_id` en token y ALLOC), bidireccionalidad de políticas explícita |
| [`a0f8ac7`](https://github.com/Tutitoos/atenea-one/commit/a0f8ac71b5e83607603c45b46113a2aa0ea607b6) (HEAD) | Carga y presupuestos, soak local con hallazgo conocido, cubos de cookie por par, parada acotada del agente, matriz de aceptación y documento de soporte |

Pruebas añadidas o conservadas que cubren temas de los hallazgos (nombre → primer commit que lo introduce según `git log -S`; VERIFICADO que la función existe en HEAD):

| Tema | Prueba | Ubicación | Commit |
| --- | --- | --- | --- |
| A-F1/C-F1 tabla de tasas llena | `full_table_evicts_refilled_buckets_under_rotation` | [`apps/relay/src/limits.rs`](../../apps/relay/src/limits.rs) | [`53351b6`](https://github.com/Tutitoos/atenea-one/commit/53351b6e0b63ce02b3eabae51e5c5b2e7f1e2b79) |
| A-F2 reloj | `observed_time_never_passes_the_validated_lease` | [`apps/agent/src/state.rs`](../../apps/agent/src/state.rs) | [`53351b6`](https://github.com/Tutitoos/atenea-one/commit/53351b6e0b63ce02b3eabae51e5c5b2e7f1e2b79) |
| A-F2 recuperación | `reset_clock_keeps_rollback_protection` | [`apps/agent/src/state.rs`](../../apps/agent/src/state.rs) | [`53351b6`](https://github.com/Tutitoos/atenea-one/commit/53351b6e0b63ce02b3eabae51e5c5b2e7f1e2b79) |
| A-F3 retroceso del estado | `periodic_persistence_never_rolls_back_a_committed_state` | [`apps/agent/src/state.rs`](../../apps/agent/src/state.rs) | [`53351b6`](https://github.com/Tutitoos/atenea-one/commit/53351b6e0b63ce02b3eabae51e5c5b2e7f1e2b79) |
| A-F4 mensaje 2 falso | `forged_responses_do_not_cancel_a_pending_handshake` | [`apps/agent/src/engine.rs`](../../apps/agent/src/engine.rs) | [`53351b6`](https://github.com/Tutitoos/atenea-one/commit/53351b6e0b63ce02b3eabae51e5c5b2e7f1e2b79) |
| A-F5 sync sin verificar | `unverified_body_never_replaces_the_active_config` | [`apps/agent/src/sync.rs`](../../apps/agent/src/sync.rs) | [`53351b6`](https://github.com/Tutitoos/atenea-one/commit/53351b6e0b63ce02b3eabae51e5c5b2e7f1e2b79) |
| A-F8/B-F8 enlaces simbólicos | `private_reads_refuse_symbolic_links` | [`apps/agent/src/platform/private_files.rs`](../../apps/agent/src/platform/private_files.rs) | [`53351b6`](https://github.com/Tutitoos/atenea-one/commit/53351b6e0b63ce02b3eabae51e5c5b2e7f1e2b79) |
| B-F1 IPC silencioso | `silent_clients_do_not_stall_packets_timers_or_control` | [`apps/agent/src/ipc.rs`](../../apps/agent/src/ipc.rs) | [`53351b6`](https://github.com/Tutitoos/atenea-one/commit/53351b6e0b63ce02b3eabae51e5c5b2e7f1e2b79) |
| B-F2 destinos de relay | `relay_destinations_are_vetted_and_bounded` | [`apps/agent/src/coord.rs`](../../apps/agent/src/coord.rs) | [`53351b6`](https://github.com/Tutitoos/atenea-one/commit/53351b6e0b63ce02b3eabae51e5c5b2e7f1e2b79) |
| C-F2 IP tras proxy | `ip_cliente_solo_se_toma_de_proxy_confiable` | [`apps/control-plane/tests/abuse.rs`](../../apps/control-plane/tests/abuse.rs) | [`53351b6`](https://github.com/Tutitoos/atenea-one/commit/53351b6e0b63ce02b3eabae51e5c5b2e7f1e2b79) |
| C-F3 login/flujos | `login_limitado_por_ip_y_con_tope_de_flujos`, `purga_flujos_y_sesiones_caducados`, `tope_de_flujos_oidc_es_duro_bajo_concurrencia` | [`apps/control-plane/tests/abuse.rs`](../../apps/control-plane/tests/abuse.rs) | [`53351b6`](https://github.com/Tutitoos/atenea-one/commit/53351b6e0b63ce02b3eabae51e5c5b2e7f1e2b79) |
| C-F4 nonces/polls | `device_nonce_quota_preserves_other_network`, `device_poll_quota_preserves_other_network_and_cancellation` | [`apps/coordinator/tests/api.rs`](../../apps/coordinator/tests/api.rs) | [`53351b6`](https://github.com/Tutitoos/atenea-one/commit/53351b6e0b63ce02b3eabae51e5c5b2e7f1e2b79) |
| C-F5 endpoints firmados | `config_limitada_por_dispositivo_y_auditoria_acotada`, `dispositivo_pendiente_no_toma_el_bloqueo_de_la_red`, `estado_con_firma_invalida_no_bloquea_la_fila_del_dispositivo` | [`apps/control-plane/tests/abuse.rs`](../../apps/control-plane/tests/abuse.rs) | [`53351b6`](https://github.com/Tutitoos/atenea-one/commit/53351b6e0b63ce02b3eabae51e5c5b2e7f1e2b79) |
| C-F7 CIDR de red | `cidr_de_red_debe_ser_privado_y_no_demasiado_amplio` (API y restricción SQL, migración [`0004_network_cidr_private.sql`](../../apps/control-plane/migrations/0004_network_cidr_private.sql)) | [`apps/control-plane/tests/abuse.rs`](../../apps/control-plane/tests/abuse.rs) | [`53351b6`](https://github.com/Tutitoos/atenea-one/commit/53351b6e0b63ce02b3eabae51e5c5b2e7f1e2b79) |
| C-F12 endpoints | `endpoints_fijados_se_sanean` | [`apps/control-plane/tests/abuse.rs`](../../apps/control-plane/tests/abuse.rs) | [`53351b6`](https://github.com/Tutitoos/atenea-one/commit/53351b6e0b63ce02b3eabae51e5c5b2e7f1e2b79) |
| C-F14 | `token_sin_cache_y_revocacion_idempotente` | [`apps/control-plane/tests/abuse.rs`](../../apps/control-plane/tests/abuse.rs) | [`53351b6`](https://github.com/Tutitoos/atenea-one/commit/53351b6e0b63ce02b3eabae51e5c5b2e7f1e2b79) |
| A-F1 (motor, wire v2) | `invalid_mac1_never_spends_dh_or_origin_state` | [`apps/agent/src/engine.rs`](../../apps/agent/src/engine.rs) | [`d9ab7ec`](https://github.com/Tutitoos/atenea-one/commit/d9ab7ec4b4af44dd022ab655b7747c16231a6f8b) |
| T30 | `relay_cookie_is_bound_to_id_direction_and_pending_transport` | [`apps/agent/src/engine.rs`](../../apps/agent/src/engine.rs) | [`d9ab7ec`](https://github.com/Tutitoos/atenea-one/commit/d9ab7ec4b4af44dd022ab655b7747c16231a6f8b) |
| A-F1 (cubo por par) | `configured_cookie_reservations_complete_under_unknown_load_direct_and_relay` | [`apps/agent/src/engine.rs`](../../apps/agent/src/engine.rs) | [`a0f8ac7`](https://github.com/Tutitoos/atenea-one/commit/a0f8ac71b5e83607603c45b46113a2aa0ea607b6) |
| B-F4/B-F5 | `test_environment.py`, `test_sbom.py` | [`tests/release/test_environment.py`](../../tests/release/test_environment.py), [`tests/release/test_sbom.py`](../../tests/release/test_sbom.py) | [`53351b6`](https://github.com/Tutitoos/atenea-one/commit/53351b6e0b63ce02b3eabae51e5c5b2e7f1e2b79) |

- **PARCIAL**: existen pruebas con nombre para la mayoría de temas Medios/Alto. Sin prueba localizada por nombre en esta revisión (SIN COMPROBAR): A-F11 (sustitución de ancla siguiente), A-F9 (`device_id` no firmado), B-F3 (DNS residual macOS/Windows tras `kill -9`: la propia T28 admite que solo hay ensayo Linux), B-F6, B-F10–B-F12, B-F14, C-F6 (decisión de aceptación, ver §6), C-F8, C-F9, C-F10 y C-F11.
- El control plane tiene pruebas contra PostgreSQL que, según su forma (`let Some(t) = TestApp::new().await else { return; }`), **retornan sin fallar si no hay base de datos disponible**. VERIFICADO por lectura del código; INFERIDO que, sin PostgreSQL, esas pruebas no ejercitan nada. El auditor debe confirmar en su ejecución que no se omiten.
- Las tablas de línea («~») de los informes y los números exactos de línea de §3 de cada informe son del árbol previo; **no** usarlos como localizadores actuales.

## 5. Decisiones de diseño posteriores a la revisión

Fuente normativa: [modelo de amenazas](../security/threat-model.md), [wire-v2](../protocol/wire-v2.md), [relay-v2](../protocol/relay-v2.md), [matriz](../operations/matriz-de-aceptacion.md).

| Decisión | Resumen (VERIFICADO en `threat-model.md` T9/T10/T16/T18/T20/T21/T30 y `soporte-limites-y-recuperacion.md`) | Estado de la evidencia |
| --- | --- | --- |
| **Wire v2** | Cutover limpio, sin negociación mixta ni shim con v1. Orden de comprobación: versión/tamaño → MAC1 → contador monotónico (umbral por defecto 100/s) → bajo carga MAC2/cookie → cupos por origen → DH. MAC1 inválido no toca estado por origen. RESP y COOKIE_REPLY de 62 B frente a INIT de 146 B (T9). | Implementada (prueba `invalid_mac1_never_spends_dh_or_origin_state`); revisión de seguridad independiente SIN COMPROBAR |
| **Relay v2** | `relay_id` de 16 B en el token (189 B) y firmado en ALLOC; el relay exige igualdad con su configuración; ALPN exclusivo `atenea-relay/2`; v1 sin retrocompatibilidad (T18, T21). | La propia T21 contiene el texto «Especificado, no implementado/verificado»: es una frase histórica anterior a `d9ab7ec`, y la matriz y el soporte documentan ALPN `/2` aceptado y `/1` rechazado en laboratorio (LOCAL-V2). **Inconsistencia documental que el auditor debe resolver contra el código**; PARCIAL |
| **Cookies por par (T10/T30)** | Reserva de respuestas independiente de los desconocidos: 2/s con ráfaga 4 por par configurado; desconocidos 20/s ráfaga 20 por IP (IPv6 /64), global 100/s ráfaga 100, tabla ≤ 4096 recuperable. La cookie no autentica ni autoriza: solo Noise y la configuración firmada lo hacen. Respuesta con XChaCha20Poly1305, secretos que rotan cada 120 s. | Implementada en `a0f8ac7`; medición con carga en Docker/Mac, no un VPS ([rendimiento](../operations/rendimiento-y-presupuestos.md)) |
| **Parada acotada del agente** | `atenea down` hasta desaparición real del proceso en < 3 s: 21/21 invocaciones, máximo 2,079 s (MEDIDO local, documento de rendimiento). El IPC se atiende fuera del bucle de datos (T27). | MEDIDO en laboratorio; no atribuible a máquinas reales |
| **Bidireccionalidad de políticas (C-F6)** | Se mantiene la regla inversa sin puertos y se documenta en panel y contrato (decisión aceptada el 2026-10-25). | **Riesgo aceptado**, no corregido (§6) |
| **Exclusión de v1** | HTTP API `/v1` y versiones de configuración firmada son espacios distintos del cable; no se retiran por el nombre. | Documental |

## 6. Hallazgos abiertos y riesgos aceptados (a este HEAD)

«Mitigado» = cobertura técnica propia; **no** riesgo eliminado ni validado por un tercero. Resumen de la [matriz](../operations/matriz-de-aceptacion.md#hallazgos-abiertos-y-riesgos-aceptados), que repite T1–T30 línea a línea; aquí solo los puntos que afectan a este informe.

### 6.1 Aceptados explícitamente por el responsable (2026-10-25, según el modelo de amenazas)

| Tema | Amenaza | Condición de salida declarada |
| --- | --- | --- |
| ACL sin estado y regla inversa sin puertos (C-F6) | T1, T26 | ACL con seguimiento de conexiones en el agente, retirar la regla inversa general, verificar que el retorno funciona y que las conexiones nuevas en sentido inverso se deniegan salvo política. Hasta entonces **no hay aislamiento unidireccional** |
| Roaming sin validación de retorno (A-F7) | T17, T30 | Ninguna fijada; se acepta como límite tipo WireGuard. Mitigación parcial: no se adopta un endpoint dentro de la red del túnel |
| DNS dividido con dominios públicos admitidos (B-F7) | ADR-007 §5 | Pendiente de decidir aviso en el panel o lista de permitidos por red |
| Coordinator comprometido puede provocar tráfico acotado hacia terceros | T16 | Sin condición; una cookie no autentica al destinatario |
| Clonación de VM/snapshot reutiliza sesión | T14 | Sin prueba; «revisar con el auditor» |
| Sin anonimato ni relleno; metadatos visibles | T15 | Fuera de objetivo |

### 6.2 Abiertos o parciales relevantes

- **T4 MFA** (C-F9): parcial. La mitigación depende del IdP del operador; la pila de referencia con Dex es de un factor y no se comprueba `acr/amr`. PARCIAL.
- **T6 raíz**: el ensayo de rotación con dos anclas y la reincorporación no están completos; token hardware pendiente de elección ([ADR-005](../architecture/ADR-005-custodia-clave-raiz.md), matriz). A-F11 (la raíz vigente puede sobrescribir la sucesora anunciada) no tiene contramedida identificada en esta revisión: SIN COMPROBAR.
- **T10/T20 disponibilidad**: cuotas implementadas; los límites no garantizan disponibilidad frente a fuentes independientes ni hay HA ([soporte](../operations/soporte-limites-y-recuperacion.md)).
- **T21**: replay de ALLOC dentro de la misma audiencia y frescura sigue permitido; IDs duplicados comparten audiencia.
- **T25, T27, T28**: conflictos de LAN solo al aplicar; el grupo de estado puede ocupar las plazas de su listener; en macOS/Windows el DNS residual persiste tras una parada forzada hasta reinicio del servicio o `cleanup` (B-F3, ensayo físico pendiente).
- **T29**: el entorno `release` y la protección de ramas/tags **no** son demostrables desde el repo; PKG/ZIP sin firma nativa; la clave de firma es un secreto de GitHub. La CI atestigua lo que pasó por el workflow, no una construcción reproducible. SIN COMPROBAR contra GitHub real.
- **Fuzzing**: A señaló que `signed_config_verify` casi nunca superaba la verificación de firma y que faltaba un objetivo para la máquina de estados; `53351b6` añade `agent_engine` y rehace `signed_config_verify` (VERIFICADO por los ficheros del commit). La primera ejecución de los 13 objetivos se registra en `fuzz/README.md` (`954a503`); no repetida por mí. Resultados de cobertura y regresiones: PARCIAL.

### 6.3 Evidencia operativa: corregidos tras revisión y límites abiertos

**Corregidos tras revisión** (resultados comunicados del laboratorio; no ejecutados para esta actualización):

- **Control plane**: aplicado el índice de `ip_allocations` y medido de extremo a extremo en laboratorio con la misma matriz (`atenea-load-97970`, 0 errores): a 10 000 dispositivos en 79 redes y 200 clientes, p95 de lectura 15,2 → 0,43 s y de emisión 19,5 → 1,41 s; el rendimiento deja de caer con la población. No es un A/B aislado (línea base de otra sesión) ni un VPS ([rendimiento](../operations/rendimiento-y-presupuestos.md), [diagnóstico](../operations/diagnostico-control-plane-escala.md)).
- **Relay y presencia**: renovación anticipada con refresco in situ y reintento de presencia aplicados. El soak `tests/load/out/atenea-load-74335` duró 720 s con 72 muestras: no hubo cambios de `relay_down`/`relay_ready` tras t≈5,4 s en ambos agentes; b renovó a t≈245,5/470/710 s, con `attempts`/`ok` 0→1→2→3 y `failed`/`throttled` en 0. El anterior `48076` registró caídas a ≈305/605 s, hueco de 406 ms y pérdida de ping de 0,0847 %.
- **Resultado y procedencia del soak corregido**: 12 SHA íntegros, media 24,99 Mbit/s, ping 3515/3513 (pérdida 0,0569 %), RTT mín./media/máx. 0,233/1,248/33,578 ms. Las pérdidas en secuencias 882/1769 (≈176/354 s) no coinciden con las renovaciones; `path_lost` a 40 s y `probe_failed` a 50/90/160/290/545 s son esperados con el directo bloqueado. `/tmp/g-soak.txt` registra hashes de `atenea` (`aa81fc73…ed18e`) y agente (`cefdd9e5…3b327`). El ensayo `44042` usó un binario antiguo montado desde `tests/load/results/bin`, no acredita las correcciones; el stack ahora rechaza binarios anteriores a fuentes y registra SHA-256. Detalle en [evidencia](evidencia.md#correcciones-posteriores-y-evidencia-operativa) y [rendimiento](../operations/rendimiento-y-presupuestos.md).
- **Reconexión tras reinicio: corregida y medida en laboratorio**. `tests/load/out/handshake-after/handshake.jsonl`: 63 filas/tiempos, 21 experimentos (3 controles y 3 modos: `invalid-mac1`, `valid-mac1-no-cookie`, `valid-cookie` × 1k/10k/50k × 2 repeticiones), todos OK, **0 fallos**, máximo **5,317 s**. a-c queda en 5,05–5,32 s en 21/21 (mínimo exacto 5,0546 s); el residuo ≈5 s coincide con el rendezvous por par no dirigido de 5 s del coordinator, cuyo diseño no cambió. Parada <3 s en 21/21 (máximo 2,084 s). Se usaron los mismos hashes de `/tmp/g-soak.txt` que `74335`, con imágenes de coordinator/relay reconstruidas; las salidas son locales, no verificables desde un clon.
- **Cautela de comparación**: los 14 controles históricos proceden de sesiones/arnés heterogéneos: a-c ≈20 s en 14/14 y a-b/c-b a menudo rápidos con colas de 15–20 s. **No es un A/B controlado ni permite adjudicar cada mejora a una corrección**. El atípico histórico de 40,6 s con MAC1 válido sin cookie a 50k no se reprodujo en las dos repeticiones actuales (50k R2: a-b/c-b 3,56/3,66 s; a 1k hasta 1,32 s), sin demostrar su ausencia futura. Detalle en [evidencia](evidencia.md#correcciones-posteriores-y-evidencia-operativa).

**Abiertos o parciales**:

- **Soak largo**: el corregido cubre **12 min, no 72 h**. El de 72 h no se ha ejecutado; no se declara aceptación del soak ni auditoría independiente.
- **Rendimiento a escala**: control plane con 10 000 dispositivos **repartidos en 79 redes de hasta 128** (máximo 256 por red) y 200 clientes: p95 histórico ≈15–19,5 s; tras el índice, p95 ≈0,43 s (lectura) y ≈1,41 s (emisión), sin errores (no A/B aislado; laboratorio Docker/Mac). A 16 pares agregados por relay se perdió el 82–87 % con CPU ≈un núcleo.
- **Multipar**: dos defectos de sondeo corregidos con pruebas que fallaban antes; N=8 bloqueado pasó de 17/28 a 28/28 con `MAX_PER_IP=64`, pero la atribución al episodio histórico sigue siendo **INFERENCIA** ([caídas y leases](../operations/caidas-y-leases.md)).
- **Fallos y recuperación**: simulacros CP/BD con corte a `valid_until + 30 s` (medido, **histórico pre-v2**, no repetido tras el cutover); restauración medida con datos pequeños en Docker/Mac; destino off-node de copias y ensayo de pérdida simultánea sin comprobar ([copias](../operations/copias-y-recuperacion.md), [soporte](../operations/soporte-limites-y-recuperacion.md)).
- **Procedencia de resultados LOCAL-V2**: las salidas `/tmp/v2-*.txt` **no están versionadas** y se ejecutaron sobre un árbol previo a `d9ab7ec` con ediciones entre tandas; solo la CI se atribuye a ese commit. Datos citados por la matriz, no verificables desde un clon.
- **CI 40228501883**: la matriz lo cita como verde sobre `d9ab7ec` (check, panel, macos, windows, supply-chain). No consta CI sobre `a0f8ac7`; no la he consultado (sin red): SIN COMPROBAR.

## 7. Excepciones de dependencias (`deny.toml`)

VERIFICADO leyendo [`deny.toml`](../../deny.toml) y [DEP-2026-01/02 del modelo de amenazas](../security/threat-model.md#riesgos-aceptados-temporalmente-dependencias):

| ID | Paquete | Uso declarado | Salida | Revisar |
| --- | --- | --- | --- | --- |
| `RUSTSEC-2026-0387` | `crypto-bigint 0.5.5` (vía `openidconnect` → `p256`/`p384`) | Solo verificación de ID tokens (`apps/control-plane/src/auth.rs::callback`), sin secretos ECDSA/DH | `openidconnect` estable con `p256`/`p384` 0.14 y `crypto-bigint` ≥ 0.7.6; retirar la excepción | **antes de 2027-01-23** |
| `RUSTSEC-2023-0071` | `rsa 0.9.10` | Solo verificación con clave pública RSA del IdP; la clave privada de `apps/control-plane/tests/oidc.rs` es efímera y de pruebas | Versión corregida compatible o retirada upstream; sin parche acreditado hoy | **antes de 2027-01-23** |

- Cualquier uso nuevo de claves privadas ECDSA/DH/RSA con estos crates invalida anticipadamente la aceptación.
- El resto de advisories, `yanked = "deny"`, licencias permitidas (lista cerrada) y fuentes (`unknown-git = "deny"`, solo crates.io) siguen activos; el grafo excluye dependencias de desarrollo (`exclude-dev = true`).
- La afirmación «sin secretos en el proceso» es una declaración de uso, **PARCIAL**: el grafo prueba que los crates están en producción (ver abajo), no que no firmen; eso exige revisar el uso, y el auditor debe comprobar que `rsa`/`p256`/`p384` no se usan para firmar en producción.

### Producción frente al informe histórico (rsa)

- **Informe histórico (frente B, B-F5), VERIFICADO en su texto:** vio `rsa 0.9.10` en `Cargo.lock` y lo atribuyó solo a `[dev-dependencies]` del control plane. **Esa atribución está incompleta/obsoleta.**
- **Grafo exacto, VERIFICADO leyendo [`dist/audit/dependencies.json`](../../dist/audit/dependencies.json)** (generado para `a0f8ac71b5e83607603c45b46113a2aa0ea607b6`, alcance declarado «cargo metadata; incluye desarrollo y todas las plataformas»; 452 paquetes; cada dependencia con nombre, versión y origen): `openidconnect 4.0.1` (registro crates.io) depende de `rsa 0.9.10`, `p256 0.13.2` y `p384 0.13.1`; estos dos últimos dependen de `elliptic-curve 0.13.8`, y `crypto-bigint 0.5.5` aparece en el grafo. Los únicos paquetes que dependen de `openidconnect` o `rsa` son el propio `atenea-control-plane` (workspace), `jsonwebtoken 10.4.0`, `elliptic-curve`, `sqlx-mysql` y `openidconnect`.
- **Contraste con el código, VERIFICADO por lectura:** [`apps/control-plane/Cargo.toml`](../../apps/control-plane/Cargo.toml) declara `openidconnect = "4"` en dependencias normales (línea 24) y `rsa = { version = "0.9", features = ["pem"] }` solo en `[dev-dependencies]` (línea 42); [`apps/control-plane/src/auth.rs`](../../apps/control-plane/src/auth.rs) importa `openidconnect` (línea 7). [`apps/control-plane/tests/oidc.rs`](../../apps/control-plane/tests/oidc.rs) es el único uso directo de `RsaPrivateKey` (línea 34: clave RSA efímera de pruebas).
- **Conclusión:** hay **dos** orígenes de `rsa 0.9.10`. (1) el dev-dependency directo, el que vio el informe; (2) la dependencia transitiva de producción vía `openidconnect`, que el informe no contempló y que es la que justifica la excepción de `deny.toml` (cuyo `exclude-dev = true` no la elimina; sin ella `cargo deny` no habría necesitado la excepción). Coincide con el [modelo de amenazas (DEP-2026-02)](../security/threat-model.md#riesgos-aceptados-temporalmente-dependencias). El informe histórico es por tanto obsoleto en este punto y no es un bloqueante.
- **Límite, INFERIDO:** `dependencies.json` no distingue aristas normales de dev; que `rsa` llegue a producción se deduce de que `openidconnect` es dependencia normal (Cargo.toml) y lista `rsa` entre las suyas. No se ejecutó `cargo`; que el binario solo verifique (sin firmar con RSA/ECDSA) sigue siendo una declaración de uso del proyecto, no demostrada aquí.

## 8. Condiciones de salida propuestas para la aceptación

Todas dependen de decisiones fuera de este documento; no son una afirmación de que se cumplan.

1. Auditoría independiente de protocolo, crypto, privilegios y autorización con informe versionado, alcance y resolución de críticos/altos ([matriz](../operations/matriz-de-aceptacion.md)). **No cumplida.**
2. Revalidar con wire v2 / relay v2 los simulacros de caída y lease del piloto (hoy históricos pre-v2) y archivar salidas sanitizadas para que se puedan reproducir desde un clon.
3. Soak de 72 h con el piloto autorizado y binarios de procedencia registrada; comprobar en ese horizonte la renovación anticipada corregida. El ensayo de 12 min no cumple esta condición.
4. Confirmar en GitHub el environment de `release` (revisores, solo tags) y decidir el custodio hardware de la clave de firma.
5. Retirar las dos excepciones de `deny.toml` antes de 2027-01-23 o renovarlas con justificación nueva.
6. Implementar o reevaluar ACL con seguimiento de conexiones (C-F6) y decidir sobre dominios DNS públicos (B-F7).
7. Ensayo físico macOS/Windows (DNS residual, instalación, firma nativa) y sobre proveedor real (off-node, DNS/ACME públicos).
8. Resolver las inconsistencias documentales marcadas (T21 «no implementado», tamaños de mensaje heredados). La divergencia sobre `rsa` queda explicada en §7 con el grafo de `dist/audit/dependencies.json`.

## 9. Qué NO hice

- No ejecuté pruebas, builds, fuzz ni CI. No consulté GitHub.
- No releí cada corrección línea a línea: el emparejamiento hallazgo→corrección es por informe, mensaje de commit y nombre de prueba.
- No he reproducido ningún hallazgo ni confirmado los números de severidad más allá de las cabeceras de cada informe.
