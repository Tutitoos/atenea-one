# Laboratorio de red (fase 2)

Dos agentes Linux (A y B) y un tercero no autorizado (C) en namespaces unidos por un puente, dentro de un contenedor privilegiado y desechable. No toca la red del host.

```sh
tests/network-lab/run.sh          # requiere Docker; compila, prueba y ejecuta los casos
```

Casos cubiertos: ping y TCP/UDP íntegros en IPv4 e IPv6; MTU documentada (1400); captura externa sin payload en claro (con control positivo); replay, alteración de contador y datagrama sobredimensionado; peer no autorizado; pérdida, reordenación y latencia con `netem`; reinicio de un agente; parada limpia sin interfaz ni rutas residuales.

El script termina con código distinto de cero si falla algún caso. Los datos son sintéticos y las claves se generan en cada ejecución.
