---
title: Cómo funciona
description: Arquitectura de Atenea One. Control plane, coordinator, relays y agentes; cómo se conectan los dispositivos y cómo se firma y revoca la configuración.
---

## Componentes

- **Control plane y panel web.** Crea redes, aprueba dispositivos, rutas y políticas. Usa OIDC para la autenticación de administradores.
- **Emisor.** Firma las configuraciones de cada dispositivo con un certificado de emisor acotado en tiempo y serial.
- **Coordinator y STUN.** Ayudan a dos dispositivos a encontrarse (rendezvous) y a atravesar NAT. No reciben claves de los pares.
- **Relay.** Reenvía datagramas cifrados cuando no hay camino directo, por UDP o por TLS sobre TCP 443 (compartido con el panel tras un proxy de entrada).
- **Agente.** Corre en cada dispositivo, abre la interfaz del túnel y aplica la configuración firmada.

El proyecto no usa WireGuard como protocolo ni depende de servicios externos de coordinación.

## Cómo se conectan dos dispositivos

1. Cada agente descarga su configuración firmada: lista de pares, rutas aprobadas y política.
2. El coordinator publica pistas de endpoint. Son **pistas sin autenticar**: solo un handshake válido crea una ruta.
3. Los agentes intentan una conexión directa con hole punching.
4. Si no se logra, usan el relay UDP y, si la red lo bloquea, TLS sobre el puerto 443.
5. Un camino solo se considera sano con paquetes autenticados por Noise; al volver a un camino mejor hay una histéresis de 5 s para evitar oscilaciones.

## Cifrado

Los dispositivos negocian sesiones con el patrón **Noise IK**, con antirreplay (ventana de 2048) y rekey propios. El tráfico va cifrado de extremo a extremo: el relay y el coordinator no pueden leerlo ni alterarlo.

## Autorización y revocación

La configuración firmada es la fuente de autorización. La política es de denegación por defecto y se aplica en origen y destino. La **revocación** funciona mediante *leases*: una configuración tiene vigencia limitada y un dispositivo revocado deja de recibir renovaciones. El agente guarda un estado de versión y hora máxima y falla cerrado ante un reloj o una configuración anómalos.

Una regla A→B con un puerto concreto también permite a B iniciar tráfico hacia A: la regla inversa no está restringida por puertos. Es una semántica aceptada y documentada.

## Subredes y DNS

Un router Linux puede servir una subred privada completa. Un administrador aprueba cada ruta y el SNAT se elige de forma explícita. Solo se admiten prefijos privados; nunca `/0` ni salida general a Internet. El DNS dividido se aplica por enlace con systemd-resolved y no se edita `/etc/resolv.conf`.

## Más

El detalle de cada decisión vive en los ADR del repositorio, en `docs/architecture/`.
