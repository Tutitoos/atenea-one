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
| `datagram_parse` | `parse` no entra en pánico; lo aceptado se recodifica idéntico |
| `replay_window` | `check`/`update` coinciden con un modelo exacto para secuencias arbitrarias |
| `handshake_init` | `read_init` con clave fija sobre bytes arbitrarios no entra en pánico |
| `session_decrypt` | `decrypt` no entra en pánico; un fallo no avanza la ventana ni confirma la sesión |
| `hex_decode` | `hex::decode` coincide con un modelo y hace ida y vuelta |
| `signed_config_verify` | Entrada estructurada: el arnés **firma** con claves fijas de fuzz (certificado y cuerpo arbitrario), de modo que se alcanzan los pasos posteriores a la firma. Lo aceptado es canónico, del dispositivo, sin rollback de versión/serie/hora, con ≤ 2 anclas procedentes del estado o del `next_anchor`, persistible y re-aceptable de forma idempotente |
| `device_config_parse` | `DeviceConfig::parse` no entra en pánico; lo aceptado se recodifica con los mismos bytes |
| `cert_bundle_state_parse` | Parsers de `IssuerCert`, `DeviceBundle` y `AgentState`: sin pánico; lo aceptado se recodifica idéntico |
| `config_v2_parse` | Configuración v2 y dominios DNS: parseo estricto y representación única |
| `ip_classify` | `classify` y `Policy::allows` sobre paquetes arbitrarios: sin pánico; una política sin reglas deniega |
| `stun_parse` | STUN: sin pánico; lo aceptado se recodifica de forma equivalente y los candidatos saneados cumplen las reglas |
| `relay_parse` | Relay (`parse`, `RelayToken::parse`, `FrameDecoder`): sin pánico; un token aceptado se recodifica idéntico y el decodificador no acumula de más |
| `agent_engine` | Máquina de estados del agente (ADR-003): dos motores reales de `apps/agent/src/engine.rs` (se compila el fichero con `#[path]`, sin cambios en producción) sobre una red simulada con pérdida, duplicado, reordenación, corrupción, basura y relojes. La TUN solo recibe, una vez como mucho, lo que el otro extremo envió; sin lease no se entrega ni se emite tráfico de datos; todo lo emitido es un datagrama válido |

## Notas de los objetivos estructurados

- `signed_config_verify`: 57 bytes de control (serie, validez del certificado, versión/serie/hora del
  estado, `now`, banderas de rotación) y el resto es el cuerpo de la configuración, firmado tal cual. Las
  claves son fijas y públicas: **solo** para fuzzing. Las semillas (`fuzz/seeds/signed_config_verify/`)
  salen de `tests/fixtures/identity/`.
- `agent_engine`: los motores usan el RNG del sistema (índices y rekey), por lo que un fallo puede no
  reproducirse bit a bit; las aserciones no dependen de valores aleatorios. `relay.rs` no se compila (arrastra
  TLS); se sustituye únicamente `RelayKind` por una copia de dos variantes.

## Última ejecución completa

`fuzz/run.sh 90` (13 objetivos, 90 s cada uno, sin red, Docker en Mac arm64, commit `151d3d7`): **0 fallos,
0 artefactos** (`fuzz/artifacts/` vacío). Ejecuciones por objetivo, en millones: `datagram_parse` 117,5;
`stun_parse` 118,9; `ip_classify` 64,0; `hex_decode` 32,4; `relay_parse` 28,9; `device_config_parse` 27,1;
`config_v2_parse` 23,5; `cert_bundle_state_parse` 3,7; `handshake_init` 1,3; `signed_config_verify` 0,26;
`replay_window` 0,21; `agent_engine` 0,06; `session_decrypt` 0,04. Es una prueba de humo de 20 minutos, no una
campaña: el criterio de salida de la fase 11 pide fuzzing continuado (horas/días) con corpus persistente.

## Reproducir un fallo

```sh
docker run --rm -v "$PWD":/work -v atenea-fuzz-target:/work/fuzz/target atenea-fuzz \
  cargo +nightly fuzz run <objetivo> regressions/<objetivo>/<fichero>
```
