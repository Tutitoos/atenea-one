<!-- Copyright 2026 Tutitoos -->
<!-- SPDX-License-Identifier: Apache-2.0 -->
# ADR-031: Conexiones automáticas por aprobación

**Estado:** aceptado el 2026-11-20, incluida la capacidad de 500 dispositivos y el rechazo atómico de altas incompatibles. Complementa [ADR-016](ADR-016-politicas-de-acceso-minimas.md).

## Decisión de acceso

Cada red tiene un ajuste editable por su administrador: «Conectar automáticamente los dispositivos aprobados». Las redes nuevas lo tienen activo por defecto; las existentes lo reciben desactivado sin alterar su acceso. El ajuste no es una política `any↔any`.

La primera transición a aprobado, por cualquier vía, fija en la misma transacción un orden de aprobación dentro de la red y el valor del ajuste. Las aprobaciones y cambios de ajuste concurrentes se ordenan bajo el mismo bloqueo de red. Los aprobados anteriores a la migración preceden a las aprobaciones nuevas y no originan conexiones automáticas entre ellos. Ni reintentos ni reconexiones cambian esos datos.

Dos dispositivos distintos tienen conexión automática si ambos están aprobados en la misma red, el aprobado más tarde tenía activo el ajuste en su aprobación y no existe una excepción para esa pareja. Solo se guardan las excepciones, identificadas sin dirección. Quitar una conexión automática no elimina permisos manuales coincidentes: el panel debe distinguir sus orígenes. Revocar un dispositivo lo excluye por ambas vías. El permiso automático cubre los dispositivos, no concede por sí solo acceso a subredes anunciadas, funciones de administración ni a otras redes.

Con el ajuste activo, aprobar equivale a autorizar acceso entre el nuevo dispositivo y los ya aprobados. Esto también vale para la aprobación delegada y para el consumo de un token con aprobación automática. En redes segmentadas se debe desactivar el ajuste **antes** de aprobar dispositivos o entregar esos tokens. Cambiar quién puede aprobar no forma parte de esta decisión.

Ejemplo: A y B aprobados en modo manual; C aprobado tras activar el ajuste; D aprobado después de desactivarlo. Solo A–C y B–C tienen conexión automática. Quitar A–C permanece tras reconectar C. Una política manual entre A y D sigue vigente con independencia del ajuste.

## Capacidad y compatibilidad

Objetivo de aceptación: 500 dispositivos aprobados en una red con cuota y direcciones suficientes, todos con permiso mutuo. Son 499 vecinos por dispositivo y 124 750 parejas lógicas, sin insertar una política por pareja ni mantener un túnel activo con cada vecino en reposo. No se eleva por ello la cuota de una cuenta ni se amplía un rango IP elegido por el administrador.

IPAM limita hoy el número de dispositivos a 256; la emisión y el lector de configuración firmada también limitan a 256 los pares y las reglas por configuración. Cambiar solo la base de datos no basta. La configuración debe distinguir el permiso automático por vecino de las reglas manuales y mantener límites explícitos de tamaño, memoria y trabajo de verificación. La presencia en el inventario no es autorización. Una excepción no puede quedar anulada accidentalmente por una regla automática global.

Se mantiene el contrato actual para las redes y agentes que pueda representar sin pérdida. La capacidad ampliada requiere anunciar soporte para la versión nueva antes de emitirla. Si una aprobación produciría una configuración no representable para algún agente afectado, se rechaza la aprobación completa con causa accionable. Nunca se truncan pares, se abre una regla global para ahorrar espacio ni se deja sin renovación a un cliente anterior. La capacidad para volver a una versión anterior se comprueba antes del despliegue, no se presupone.

## Evidencia exigida antes de anunciar soporte

- Migración sin cambios de acceso; pruebas del ejemplo anterior, alta automática con token, concurrencia, reconexión, excepción, revocación y aislamiento entre redes y subredes.
- Con clientes mixtos, una operación no representable falla sin cambios parciales; en redes compatibles los clientes anteriores siguen renovando.
- Red sintética de 500 con 499 pares por equipo: emitir, firmar, verificar y aplicar configuraciones reales; comprobar acceso permitido y denegado y medir bytes, tiempo de emisión y aplicación, memoria y tráfico en reposo. La prueba no requiere 500 equipos físicos. Medir también renovaciones concurrentes; almacenar menos políticas no elimina el coste de distribuir los pares.
