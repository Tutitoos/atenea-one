# Par de usuario de pruebas

`atenea-testpeer` responde a eco ICMPv4 e ICMPv6 dentro de una sesión Noise IK de Atenea, sin crear interfaces, rutas ni DNS y sin privilegios. No es un agente ni un router. Usa las bibliotecas de protocolo, sesión y antirreplay del agente; no añade dependencias externas al workspace.

Crear ambos directorios con `atenea device-init --dir DIR`. El agente debe tener una configuración firmada que autorice la clave pública del par, sus direcciones de túnel y el tráfico ICMP, con `127.0.0.1:51821` como endpoint del par. El bundle indicado debe obtenerse por un canal de confianza: se verifica su prueba de posesión, no una cadena de confianza externa.

```sh
cargo run -p atenea-testpeer -- \
  --device-dir /tmp/atenea-peer \
  --agent-bundle /tmp/atenea-agent/bundle.bin \
  --listen 127.0.0.1:51821 \
  --own-addr 10.77.0.1 --own-addr6 fd77::1 \
  --agent-addr 10.77.0.2 --agent-addr6 fd77::2 \
  --network-id 00112233445566778899aabbccddeeff \
  --count 2 --timeout-secs 60
```

Lee `DIR/noise.key` (32 bytes hexadecimales; en Unix exige permisos privados), y únicamente acepta la clave Noise del `DeviceBundle` de `--agent-bundle`. IPv4 es obligatorio; ambos argumentos IPv6 se usan juntos. La escucha predeterminada es `127.0.0.1:51821`; la red predeterminada es la del ejemplo. `--count` vale 1 y `--timeout-secs` vale 60 por defecto. Con `--count 0` termina inmediatamente tras abrir la escucha.

Al abrir el socket imprime y vacía `ready` en stdout. Al terminar la escucha imprime `echo_replies=N`; el contador incluye respuestas cifradas entregadas al bucle UDP (un fallo de envío termina con error). Los descartes (`discarded=N`) y errores van a stderr. Sale con 0 al alcanzar el mínimo y con error si vence el plazo o falla E/S. Errores de configuración previos a abrir la escucha no imprimen `ready` ni el contador.

La dirección UDP se fija al primer handshake fresco de la clave autorizada: no admite migración de endpoint, candidatos externos ni relays. El agente inicia el handshake al ver tráfico para el par y también inicia el rekey; este par responde, conserva la sesión anterior durante la transición y no transmite hasta confirmar la nueva mediante un paquete autenticado. Envía keepalives cada 15 segundos sin otro envío; las sesiones respetan la caducidad dura de `atenea-crypto`.

Solo responde a peticiones de eco completas, con checksums válidos, destino propio y origen exacto del agente. Descarta todo lo demás, incluidos fragmentos IPv4, opciones IPv4 y extensiones/fragmentos IPv6. No reensambla paquetes, no inicia conexiones y no interpreta ACL/configuración firmada. Los contadores de eco no prueban recepción de la respuesta por el agente.

Pruebas sin red real ni privilegios:

```sh
cargo fmt -p atenea-testpeer --check
cargo clippy -p atenea-testpeer --all-targets -- -D warnings
cargo test -p atenea-testpeer
```

Cubren handshake, confirmación, rekey, keepalives, eco cifrado IPv4/IPv6 (verificación independiente de checksums), antirreplay, autenticación, longitudes malformadas y orígenes no autorizados. El ping por `utun` y la integración privilegiada pertenecen al runner macOS de CI, no a estas pruebas.
