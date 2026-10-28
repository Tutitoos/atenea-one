# Laboratorio de red (fase 2)

Dos agentes Linux (A y B) y un tercero no autorizado (C) en namespaces unidos por un puente, dentro de un contenedor privilegiado y desechable. No toca la red del host.

```sh
tests/network-lab/run.sh          # requiere Docker; compila, prueba y ejecuta los casos
```

Casos cubiertos: ping y TCP/UDP íntegros en IPv4 e IPv6; MTU documentada (1400); captura externa sin payload en claro (con control positivo); replay, alteración de contador y datagrama sobredimensionado; peer no autorizado; pérdida, reordenación y latencia con `netem`; reinicio de un agente; parada limpia sin interfaz ni rutas residuales.

El script termina con código distinto de cero si falla algún caso. Los datos son sintéticos y las claves se generan en cada ejecución.

## Cutover wire-v2 y relay-v2

Los agentes y relays del laboratorio deben actualizarse juntos: no hay fallback v1.
`multi-peer.py` genera una vez por montaje un `relay_id` aleatorio de 16 bytes con
el RNG del SO, lo guarda en `/tmp/lab/relay-id` y comparte sus 32 hex minúsculos
no cero entre `ATENEA_RELAY_ID` y la entrada de `ATENEA_COORD_RELAYS`.
El montaje efímero puede cambiar de ID al destruirse; reiniciar un servicio no
debe generar otra audiencia. Si se añaden relays, cada uno necesita un ID distinto
y su token propio. TLS usa exclusivamente ALPN `atenea-relay/2`.

Los resultados históricos de fases anteriores no verifican este cutover.
No se han ejecutado laboratorios ni checks durante esta actualización.

## Un cliente en dos redes (ADR-013 fase 2)

```sh
tests/network-lab/run-two-networks.sh   # requiere Docker; compila y ejecuta dos-networks en contenedores desechables
```

Control plane real + PostgreSQL 17 + Dex (como `run-e2e.sh`), un cliente (`cli`) con `atenea network join` en dos redes de CIDR distintos (A `10.78.1.0/24`, B `10.78.2.0/24`) y un peer clásico por red (`pa`, `pb`). A se une con un código **con token** (política `cerrada`) y B con el código **sin token** del panel (política `con_solicitud`); en ambos casos el administrador aprueba localizando al dispositivo por la huella que imprime el CLI. Se comprueba: claves, interfaces, puertos (51820/51821) y sockets propios; ping en las dos redes; aislamiento (con el reenvío del kernel activo, sin tráfico pa↔pb en las interfaces del cliente); salir de A sin una pérdida de ping en B y sin cambiar sus ficheros ni recursos; volver a unir A con claves nuevas y revocar B sin afectar a A; código manipulado sin nada en disco ni en el servidor; y una tercera red (otra organización, CIDR solapado con A) que acaba en estado `conflicto` sin interfaz y sin tocar A ni B.

Limitaciones honestas:

- El código exige `https://`; el control plane del laboratorio es `http`. Se inicia sesión con `PUBLIC_URL` http, se reinicia con `https://192.0.2.254:8080` (para que el servidor emita códigos válidos) y el CLI usa `--insecure-http`, que sustituye el esquema. No se prueba TLS real.
- El servidor solo rechaza CIDR solapados dentro de una organización: la red C es de otra organización y el rechazo lo hace el cliente al recibir la configuración (el código no lleva CIDR), no en `join`.
- Cada red usa su propia raíz de confianza; reutilizar una raíz entre redes (ADR-013 §5) no se prueba.
- El script asume las salidas del CLI (`red=`, `huella_dispositivo=`, `estado=`, `network list --json`, `network.json`, `ATENEA_RUN_DIR`); si el CLI cambia de formato hay que ajustarlo.
- «Ficheros intactos» compara `network.json`, `agent.toml`, claves y bundle; el estado y la configuración firmada que el agente reescribe se excluyen a propósito. El script añade `interval_secs = 10` a `[sync]` del `agent.toml` del cliente (como `e2e-cp.sh`) entre `join` y `up`.
- Coordinator y relay no intervienen; los caminos son directos por el puente `br0`. Un solo servidor y una sola máquina: no mide carga ni macOS.
- Ejecutado el 2026-10-28 en Docker (Apple silicon, arm64): `DOS REDES: TODO OK`, 117 comprobaciones sin fallos. No cubre TLS real (el CP del laboratorio es `http` y el CLI sustituye el esquema con `--insecure-http`), ni macOS, ni carga.
