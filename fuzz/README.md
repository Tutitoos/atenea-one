# Fuzzing

cargo-fuzz (nightly) dentro de Docker; no se instala nada en el host.

```sh
fuzz/run.sh              # todos los objetivos, 60 s cada uno
fuzz/run.sh 300          # todos, 300 s
fuzz/run.sh 60 hex_decode  # un objetivo
```

El código de salida es ≠ 0 si algún objetivo encuentra un fallo o si una regresión versionada vuelve a
romperse. El `target` se cachea en el volumen Docker `atenea-fuzz-target`. El corpus generado
(`fuzz/corpus/`) y los fallos (`fuzz/artifacts/`) no se commitean; solo se versionan semillas pequeñas en
`fuzz/seeds/<objetivo>/` (se copian al corpus al ejecutar) y reproductores mínimos de fallos ya corregidos
en `fuzz/regressions/<objetivo>/` (procedimiento en `fuzz/regressions/README.md`).

## Objetivos

| Objetivo | Comprueba |
|---|---|
| `datagram_parse` | Parser wire-v2 sin pánico; round-trip idéntico de INIT, RESP, DATA y COOKIE_REPLY aceptados |
| `replay_window` | `check`/`update` coinciden con un modelo exacto para secuencias arbitrarias |
| `handshake_init` | INIT146: parseo, MAC1/MAC2 y decisión `pre_dh` antes de `read_init`; COOKIE_REPLY62: sellado/apertura y apertura de bytes arbitrarios, sin pánico |
| `session_decrypt` | Recibe DATA wire-v2 completo; `decrypt` no entra en pánico y un fallo no avanza la ventana ni confirma la sesión |
| `hex_decode` | `hex::decode` coincide con un modelo y hace ida y vuelta |
| `signed_config_verify` | Entrada estructurada: el arnés **firma** con claves fijas de fuzz (certificado y cuerpo arbitrario), de modo que se alcanzan los pasos posteriores a la firma. Lo aceptado es canónico, del dispositivo, sin rollback de versión/serie/hora, con ≤ 2 anclas procedentes del estado o del `next_anchor`, persistible y re-aceptable de forma idempotente |
| `device_config_parse` | `DeviceConfig::parse` no entra en pánico; lo aceptado se recodifica con los mismos bytes |
| `cert_bundle_state_parse` | Parsers de `IssuerCert`, `DeviceBundle` y `AgentState`: sin pánico; lo aceptado se recodifica idéntico |
| `config_v2_parse` | Configuración v2 y dominios DNS: parseo estricto y representación única |
| `ip_classify` | `classify` y `Policy::allows` sobre paquetes arbitrarios: sin pánico; una política sin reglas deniega |
| `stun_parse` | STUN: sin pánico; lo aceptado se recodifica de forma equivalente y los candidatos saneados cumplen las reglas |
| `relay_parse` | Relay (`parse`, `RelayToken::parse`, `FrameDecoder`): sin pánico; un token aceptado se recodifica idéntico y el decodificador no acumula de más |
| `agent_engine` | Máquina de estados wire-v2 (ADR-003): dos motores reales de `apps/agent/src/engine.rs` sobre red simulada, pérdida/duplicado/reordenación/corrupción, relojes, INIT con MAC1 válido/inválido, desafío bajo carga, COOKIE_REPLY válido/corrupto e identidad de ruta relay UDP/TLS. La TUN solo recibe una vez lo enviado por el par; sin lease no entrega ni emite datos; todo lo emitido es un datagrama válido y MAC1 inválido no genera respuesta |
| `join_code_parse` | `JoinCode::decode` (join-code-v1) con texto UTF-8 arbitrario: sin pánico; lo aceptado respeta el máximo de 512 caracteres, se re-codifica y vuelve a decodificar al mismo valor |
| `authorization_parse` | `parse_authorization` (cabecera `Authorization: AtDevice v1 …`, control-plane-v1 §3.3) con texto UTF-8 arbitrario: sin pánico; lo aceptado tiene una única representación canónica (salvo mayúsculas del hex) y se parsea al mismo valor |
| `proxy_v2_parse` | Cabecera PROXY v2 del relay: la función real (`apps/relay/src/tcp.rs::proxy`) leyendo de un `&[u8]`. Sin pánico; solo se acepta desde loopback, con comando PROXY y TCP4 (12 bytes) o TCP6 (36), sin TLV, LOCAL ni UNSPEC; la IP es la del cuerpo y se consumen exactamente 16 + longitud bytes; un origen no loopback no lee nada |
| `relay_state` | Máquina de estados del relay (`State::process`, `remove`, `sweep`) con secuencias de operaciones firmadas por el arnés: sin pánico; todo lo emitido es un mensaje válido; sin amplificación hacia un origen UDP; sin salida hacia terceros desde un origen sin asignación; DATA solo a la pareja de la misma red; PEER_STATE a la asignación correcta; topes total, por red y por origen |

## Notas de los objetivos estructurados

- `signed_config_verify`: 57 bytes de control (serie, validez del certificado, versión/serie/hora del
  estado, `now`, banderas de rotación) y el resto es el cuerpo de la configuración, firmado tal cual. Las
  claves son fijas y públicas: **solo** para fuzzing. Las semillas (`fuzz/seeds/signed_config_verify/`)
  salen de `tests/fixtures/identity/`.
- `agent_engine`: los motores usan el RNG del sistema (índices y rekey), por lo que un fallo puede no
  reproducirse bit a bit; las aserciones no dependen de valores aleatorios. `relay.rs` no se compila (arrastra
  TLS); se sustituye únicamente `RelayKind` por una copia de dos variantes.
  El umbral de cookie del arnés es 1/s para alcanzar el desafío con dos INIT sin avanzar el reloj.
  Las operaciones 9/10/11 generan INIT, respuestas cifradas para intentos pendientes y cambios de
  disponibilidad/ID de relay. Las semillas `agent_engine/{init_mac1_valid,init_mac1_invalid,cookie_reply_valid,cookie_reply_invalid,relay_ready}`
  usan estas operaciones; los bytes Noise que falten se rellenan con cero. Los INIT de operación9
  son envoltorios con MAC1 válido, no handshakes Noise autenticados; los intentos reales proceden de TUN.
  Los COOKIE_REPLY se cifran en ejecución con el índice/MAC1 del INIT pendiente, sin fijar claves secretas
  de producción ni depender de un índice aleatorio concreto.
- `authorization_parse`: las semillas (`fuzz/seeds/authorization_parse/`) son sintéticas (claves y firmas inventadas): una cabecera válida, el hex en mayúsculas, `ts=0`, `ts` con cero a la izquierda y un campo extra. Sin ellas el objetivo no alcanza un parseo correcto: la longitud exacta de los campos hex es difícil de adivinar a ciegas.
- `proxy_v2_parse`: el primer byte decide si el origen es loopback (bit 0 = 0) y el resto es el flujo. Semillas sintéticas: TCP4 y TCP6 desde loopback, TCP4 desde un origen no loopback, LOCAL, TLV y cabecera truncada. Para probarlo se hizo `proxy` genérica sobre el lector (`AsyncRead`) y se reexportó oculta (`#[doc(hidden)]`) junto con `State`; el relay en ejecución no cambia.
- `relay_state`: el arnés tiene claves fijas y públicas (coordinator y dispositivos): **solo** para fuzzing. Las banderas de la operación ALLOC alteran relay, token (caducado, vida excesiva, bytes), cookie, `ts` y firma de la prueba de posesión. Es lento (firma Ed25519 por ALLOC): unas 370 ejecuciones/s. Control negativo hecho: con una mutación del relay que envía PEER_STATE al origen en vez de a la pareja, otra con el tope por origen desplazado en uno y otra con el origen loopback sin comprobar en PROXY, el fuzzing las detecta en segundos; sin ellas, 0 fallos.
- `join_code_parse`: las semillas (`fuzz/seeds/join_code_parse/`) son sintéticas: los dos vectores fijos de
  `docs/protocol/join-code-v1.md` §5 (con y sin token), variantes válidas normalizadas (minúsculas, sin guiones)
  y entradas inválidas (truncadas, un carácter alterado, prefijo incorrecto, más de 512 caracteres, solo
  prefijo, vacía, no ASCII). Ejecución corta del objetivo nuevo, sin tocar los demás:
  `fuzz/run.sh 30 join_code_parse` (Docker, sin red; el script copia las semillas y exige la caché de
  dependencias ya presente). Pasada real de 60 s (2026-10-28, 11,6 M ejecuciones, sin fallos) hecha sin `run.sh`, con `docker run --rm` sobre una copia del repositorio para no tocar la imagen ni los volúmenes `atenea-fuzz*`.

## Última ejecución completa

`fuzz/run.sh 90` (13 objetivos, 90 s cada uno, sin red, Docker en Mac arm64, commit `151d3d7`): **0 fallos,
0 artefactos** (`fuzz/artifacts/` vacío). Ejecuciones por objetivo, en millones: `datagram_parse` 117,5;
`stun_parse` 118,9; `ip_classify` 64,0; `hex_decode` 32,4; `relay_parse` 28,9; `device_config_parse` 27,1;
`config_v2_parse` 23,5; `cert_bundle_state_parse` 3,7; `handshake_init` 1,3; `signed_config_verify` 0,26;
`replay_window` 0,21; `agent_engine` 0,06; `session_decrypt` 0,04. Es una prueba de humo de 20 minutos, no una
campaña: el criterio de salida de la fase 11 pide fuzzing continuado (horas/días) con corpus persistente.

La ejecución anterior es histórica, anterior al cutover wire-v2/relay-v2; no verifica los objetivos ni
las semillas v2 modificados. Esta integración no ejecutó compilación, fuzzing ni Docker.

## Reproducir un fallo

```sh
docker run --rm -v "$PWD":/work -v atenea-fuzz-target:/work/fuzz/target atenea-fuzz \
  cargo +nightly fuzz run <objetivo> regressions/<objetivo>/<fichero>
```

## Objetivos añadidos tras la auditoría de 2026-11-13

`authorization_parse`, `proxy_v2_parse` y `relay_state` (2026-11-14, `fuzz/run.sh` con las semillas): 90 s, 90 s y 400 s sin fallos (16,3 M, 18,8 M y 0,15 M ejecuciones). Es una pasada corta, no una campaña; la anterior sigue siendo histórica.
