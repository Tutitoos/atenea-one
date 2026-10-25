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
