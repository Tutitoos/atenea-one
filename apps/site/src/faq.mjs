// Copyright 2026 Tutitoos
// SPDX-License-Identifier: Apache-2.0

// Las preguntas de la portada. Viven aquí y no en el componente porque el
// JSON-LD `FAQPage` y la lista visible tienen que decir exactamente lo mismo.
// Cada respuesta sale de `README.md` o de `docs/` del repositorio.
export const FAQ = [
  {
    question: "¿Usa WireGuard?",
    answer:
      "No. El protocolo es propio: handshake Noise IK, antirreplay y rekey propios, sobre UDP o TCP. El proyecto no usa WireGuard como protocolo.",
  },
  {
    question: "¿Depende de algún servicio externo?",
    answer:
      "No. El coordinator, el servidor STUN y los relays son tuyos. No hay dependencia operativa de ZeroTier, Tailscale, Headscale ni de otro servicio externo de coordinación.",
  },
  {
    question: "¿Equivale a ZeroTier?",
    answer:
      "No se promete equivalencia completa. Hoy es un MVP administrable en laboratorio, sin L2 ni salida a Internet.",
  },
  {
    question: "¿Qué pasa si dos dispositivos no se alcanzan directamente?",
    answer:
      "Prueban primero un camino directo con hole punching. Si no se logra, usan un relay UDP y, si la red bloquea UDP, TLS sobre el puerto 443. Un camino solo se considera sano con paquetes autenticados.",
  },
  {
    question: "¿Puede un relay ver mi tráfico?",
    answer:
      "No el contenido: va cifrado de extremo a extremo. Sí ve direcciones IP, tamaños y tiempos de los paquetes, y puede descartarlos o retrasarlos. No se pretende anonimato frente al relay.",
  },
  {
    question: "¿En qué sistemas funciona?",
    answer:
      "Cliente para Linux (Debian 13 y Ubuntu 24.04 o posterior), macOS 11 o posterior y Windows 10/11 x64. El router que sirve subredes es siempre Linux.",
  },
  {
    question: "¿Cómo se revoca un dispositivo?",
    answer:
      "Con leases: la configuración firmada tiene vigencia limitada y un dispositivo revocado deja de recibir renovaciones. El corte llega, como máximo, al vencer el lease vigente.",
  },
  {
    question: "¿Puedo usarlo con datos sensibles?",
    answer:
      "Todavía no. El protocolo no tiene revisión criptográfica independiente y los paquetes de macOS y Windows se publican sin firma de desarrollador. Es un producto en evaluación.",
  },
  {
    question: "¿Qué licencia tiene?",
    answer: "Apache-2.0. El código es público en GitHub.",
  },
  {
    question: "¿Cómo reporto una vulnerabilidad?",
    answer:
      "Con Security → Report a vulnerability en el repositorio de GitHub, que abre un aviso privado. No publiques detalles explotables en una issue.",
  },
];
