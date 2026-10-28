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

El caso también inscribe `pa` con un token `role=server` y contrasta los roles
en la API de administración y en los sockets de estado de los agentes reales.
Espera con plazo la sincronización de roles y la lectura inicial de `/server/info`:
versión y commit deben coincidir con `atenea-control-plane --version`, el certificado
del emisor debe tener caducidad numérica vigente y `last_error` debe ser null.
Después comprueba que una firma válida del propio dispositivo (con control positivo
en `/device/peers`, sin cookies de administrador) no permite cambiar su rol por
PATCH admin, y que el cambio autorizado a `device` aparece tras sincronizar.
No incluye un servidor antiguo sin estos endpoints: no añade una simulación.
La compilación usa cachés existentes y `--locked --offline`, sin descargar dependencias.

Limitaciones honestas:

- El código exige `https://`; el control plane del laboratorio es `http`. Se inicia sesión con `PUBLIC_URL` http, se reinicia con `https://192.0.2.254:8080` (para que el servidor emita códigos válidos) y el CLI usa `--insecure-http`, que sustituye el esquema. No se prueba TLS real.
- El servidor solo rechaza CIDR solapados dentro de una organización: la red C es de otra organización y el rechazo lo hace el cliente al recibir la configuración (el código no lleva CIDR), no en `join`.
- Cada red usa su propia raíz de confianza; reutilizar una raíz entre redes (ADR-013 §5) no se prueba.
- El script asume las salidas del CLI (`red=`, `huella_dispositivo=`, `estado=`, `network list --json`, `network.json`, `ATENEA_RUN_DIR`); si el CLI cambia de formato hay que ajustarlo.
- «Ficheros intactos» compara `network.json`, `agent.toml`, claves y bundle; el estado y la configuración firmada que el agente reescribe se excluyen a propósito. El script añade `interval_secs = 10` a `[sync]` del `agent.toml` del cliente (como `e2e-cp.sh`) entre `join` y `up`.
- Coordinator y relay no intervienen; los caminos son directos por el puente `br0`. Un solo servidor y una sola máquina: no mide carga ni macOS.
- Ejecutado el 2026-10-28 en Docker (Apple silicon, arm64): `COMPROBACIONES: 127; pasan: 127; fallan: 0`, `DOS REDES: TODO OK`. Incluye diez comprobaciones nuevas de roles e información del servidor. El PATCH firmado por dispositivo devuelve `401 invalid_session`; el mismo firmante accede a `/device/peers` con HTTP 200. La primera ejecución tuvo tres fallos del script al interpretar `device_id` como texto en lugar de bytes y se interrumpió con un error de sintaxis después de editar el script mientras estaba ejecutándose. Se corrigió solo el script y la segunda ejecución completó todos los casos. No cubre TLS real (el CP del laboratorio es `http` y el CLI sustituye el esquema con `--insecure-http`), ni macOS, ni carga.

## Servicios Linux multiinstancia (ADR-013 fase 3)

```sh
# Reutiliza una imagen systemd previamente disponible; no la construye ni descarga.
ATENEA_PACKAGING_IMAGE=atenea-packaging tests/packaging/run.sh
# Sin descargas y sin modificar la imagen de infraestructura ya disponible:
ATENEA_PACKAGING_IMAGE=atenea-infra:local ATENEA_PACKAGING_NO_DNS=1 tests/packaging/run.sh
```

Requiere Docker Linux con cgroup v2, las imágenes locales `atenea-lab` y la indicada
por `ATENEA_PACKAGING_IMAGE`, y los volúmenes de compilación/caché ya preparados.
La imagen de empaquetado debe contener las herramientas de
`tests/packaging/Dockerfile`; no se reconstruye ni se elimina. La compilación de
los dos binarios usa `--locked --offline`: si falta la caché, falla sin descargar.
El modo explícito `ATENEA_PACKAGING_NO_DNS=1` permite reutilizar `atenea-infra:local`
sin instalar nada: omite los ensayos de DNS dividido con `systemd-resolved` y
anuncia la ausencia de `shellcheck`. El resto (systemd real, `.deb`, redes,
tráfico, actualización y retirada) es el mismo. Todas las invocaciones Docker
usan `--pull=never --network none`; la compilación usa `CARGO_BUILD_JOBS=2`.

`tests/packaging/in-container.sh` conserva las pruebas legacy y carga
`multi-network.sh` dentro del contenedor con systemd como PID 1. Genera claves y
configuraciones firmadas efímeras para legacy y dos redes, cada una con un peer
en su namespace y tráfico TCP local real. Una red usa la raíz por defecto y la
otra `/opt/atenea-sintetico/networks`, para probar también el dropin `--root`
bajo `ProtectSystem=strict`. Comprueba restricciones iguales a legacy, UUID,
condición sin configuración, sockets, interfaces y puertos distintos; down,
restart y disable de una red sin cambiar PIDs, sockets, interfaces ni cortar
conexiones TCP establecidas de legacy y la otra red; actualización Debian que
reinicia solo las previamente activas, `daemon-reexec` con habilitaciones intactas,
y remove/purge sin recursos de red residuales ni cambios en claves/configuración.
También para legacy y comprueba que las otras redes siguen comunicándose.

Ejecutado localmente en modo offline con `atenea-infra:local`: todas las pruebas
de servicio, tráfico firmado y conservación tras remove/purge terminaron en verde.
DNS dividido con systemd-resolved y shellcheck **no comprobados** en esa imagen;
siguen incluidos en el modo completo por defecto para CI. No prueba macOS, reinicio
del kernel, control plane, coordinator ni relay; `daemon-reexec` no sustituye un
reinicio completo del sistema. Todos los datos del escenario son sintéticos.
