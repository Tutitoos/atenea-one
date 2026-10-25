# Regresiones de fuzzing

Un fichero por fallo ya corregido, en `fuzz/regressions/<objetivo>/<id>`. `fuzz/run.sh` los reproduce
(`-runs=0`) antes de fuzzear cada objetivo y falla si alguno vuelve a romperse.

## Procedimiento

1. Un fallo deja su reproductor en `fuzz/artifacts/<objetivo>/crash-…` (no versionado).
2. Reproducir: `cargo +nightly fuzz run <objetivo> fuzz/artifacts/<objetivo>/crash-…`
   (dentro del contenedor, véase `fuzz/README.md`).
3. Minimizar: `cargo +nightly fuzz tmin <objetivo> <crash>`.
4. Corregir el defecto y añadir **además** una prueba unitaria o de propiedades en el crate afectado.
5. Copiar el reproductor minimizado a `fuzz/regressions/<objetivo>/` con un nombre que describa el
   defecto (`panic-longitud-cero`, no el hash), sin datos reales ni claves. Debe ser pequeño (< 4 KiB).
6. Comprobar que antes de la corrección `fuzz/run.sh 1 <objetivo>` falla y después pasa.

El formato de la entrada de cada objetivo está documentado en su cabecera
(`fuzz/fuzz_targets/<objetivo>.rs`); `signed_config_verify` y `agent_engine` usan entradas
estructuradas (bytes de control + cuerpo, u operaciones), no ficheros de protocolo.

Todavía no hay regresiones: ningún objetivo ha producido un fallo reproducible que no se haya
corregido en el mismo cambio.
