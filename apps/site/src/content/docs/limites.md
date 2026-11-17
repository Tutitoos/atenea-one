---
title: Estado y límites
description: Qué ofrece hoy Atenea One, qué no soporta y qué falta por validar.
---

**Atenea One es un MVP administrable en laboratorio. Producto en evaluación, sin auditoría independiente: no lo uses con datos sensibles.** No hay SLA ni se promete aceptación de producción por tener la integración continua en verde.

## Qué hay

- Túnel Linux cifrado entre agentes con configuración firmada, ACL y revocación por lease.
- Control plane con OIDC, coordinator y STUN propios, hole punching, relay UDP y respaldo TLS/TCP 443.
- Panel web, subredes servidas por un router Linux y DNS dividido por enlace.
- Clientes para macOS y Windows (solo cliente) y aplicación de escritorio.

## Qué no soporta

- Salida a Internet (*exit node*), ruta por defecto o subredes públicas. Solo prefijos privados.
- Servir o anunciar subredes en macOS o Windows: solo consumen rutas aprobadas y DNS dividido.
- L2, TAP o Ethernet.
- Proxies HTTP ni redes con inspección TLS corporativa. TCP sobre TCP puede degradar el rendimiento.
- Alta disponibilidad del control plane, el coordinator, la base de datos o la entrada, ni conmutación por error multirregión.
- Recuperación automática de una clave raíz perdida o comprometida: hay que reincorporar todos los dispositivos.
- Anonimato frente al relay o a un observador de red.
- Compatibilidad con versiones del protocolo anteriores: el cable es wire-v2 y relay-v2, sin negociación mixta.

## Qué falta por validar

- Auditoría criptográfica y de seguridad independiente.
- Instalación firmada y notarizada en macOS, y firma de código en Windows; hoy los paquetes se publican sin ellas.
- Matriz de interoperabilidad entre Linux, macOS y Windows en máquinas reales, incluidos suspensión y reanudación y cambios de red.
- Capacidad de producción: los límites del relay y del coordinator son valores configurados, no rendimiento medido.

Para incidencias indica artefacto, arquitectura, versión y commit.
