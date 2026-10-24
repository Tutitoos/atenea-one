# Fuzzing

cargo-fuzz (nightly) dentro de Docker; no se instala nada en el host.

```sh
fuzz/run.sh              # todos los objetivos, 60 s cada uno
fuzz/run.sh 300          # todos, 300 s
fuzz/run.sh 60 hex_decode  # un objetivo
```

El código de salida es ≠ 0 si algún objetivo encuentra un fallo. El `target` se cachea en el volumen
Docker `atenea-fuzz-target`. El corpus generado (`fuzz/corpus/`) y los fallos (`fuzz/artifacts/`) no se
commitean; solo se versionan semillas pequeñas en `fuzz/seeds/<objetivo>/` (se copian al corpus al ejecutar)
y reproductores mínimos de fallos ya encontrados en `fuzz/regressions/<objetivo>/`.

## Objetivos

| Objetivo | Comprueba |
|---|---|
| `datagram_parse` | `parse` no entra en pánico; lo aceptado se recodifica idéntico |
| `replay_window` | `check`/`update` coinciden con un modelo exacto para secuencias arbitrarias |
| `handshake_init` | `read_init` con clave fija sobre bytes arbitrarios no entra en pánico |
| `session_decrypt` | `decrypt` no entra en pánico; un fallo no avanza la ventana ni confirma la sesión |
| `hex_decode` | `hex::decode` coincide con un modelo y hace ida y vuelta |

## Reproducir un fallo

```sh
docker run --rm -v "$PWD":/work -v atenea-fuzz-target:/work/fuzz/target atenea-fuzz \
  cargo +nightly fuzz run <objetivo> regressions/<objetivo>/<fichero>
```

Los objetivos de configuración firmada se añadirán cuando exista `atenea-identity`.
