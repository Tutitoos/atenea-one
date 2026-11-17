// Copyright 2026 Tutitoos
// SPDX-License-Identifier: Apache-2.0

// La comparación con Tailscale y ZeroTier. Es de **arquitectura y protocolo**,
// no de rendimiento: en este repositorio no hay una sola medición de ninguno de
// los dos, y la página lo dice.
//
// Reglas de este fichero:
// - Toda afirmación sobre un tercero lleva la clave de su fuente, consultada en
//   la fecha `CONSULTED`. Lo que no se pudo sostener con una fuente no está:
//   por eso faltan filas como la licencia de ZeroTier o sus auditorías.
// - Lo de Atenea One sale de `docs/` (ADR-001, 002, 003, 005, 007, wire-v2,
//   relay-v2, mtu.md, threat-model.md, soporte-limites-y-recuperacion.md).
// - Una celda es `[texto, ...clavesDeFuente]`.

export const CONSULTED = "2026-11-17";

export const SOURCES = {
  tsControl: {
    label: "Tailscale: Control and data planes",
    url: "https://tailscale.com/docs/concepts/control-data-planes",
  },
  tsCrypto: {
    label: "Tailscale: Tailscale encryption",
    url: "https://tailscale.com/docs/concepts/tailscale-encryption",
  },
  tsConn: {
    label: "Tailscale: Connection types",
    url: "https://tailscale.com/docs/reference/connection-types",
  },
  tsDerp: {
    label: "Tailscale: DERP servers",
    url: "https://tailscale.com/docs/reference/derp-servers",
  },
  tsDown: {
    label: "Tailscale: What happens if the coordination server is down?",
    url: "https://tailscale.com/docs/reference/coordination-server-down",
  },
  tsLock: {
    label: "Tailscale: Tailnet Lock",
    url: "https://tailscale.com/docs/features/tailnet-lock",
  },
  tsOss: {
    label: "Tailscale: Open source at Tailscale",
    url: "https://tailscale.com/opensource",
  },
  wg: {
    label: "WireGuard: Protocol & Cryptography",
    url: "https://www.wireguard.com/protocol/",
  },
  ztProto: {
    label: "ZeroTier: The Protocol",
    url: "https://docs.zerotier.com/protocol/",
  },
  ztRules: {
    label: "ZeroTier: Rules Engine",
    url: "https://docs.zerotier.com/rules/",
  },
  ztSelf: {
    label: "ZeroTier: Self Hosting",
    url: "https://docs.zerotier.com/selfhosting",
  },
};

/** Quién opera la pieza que pone en contacto a dos dispositivos. */
export const OPERATORS = [
  {
    name: "Atenea One",
    value: "tú",
    detail: "coordinator, STUN y relays propios; sin servicio de terceros",
    highlight: true,
  },
  {
    name: "Tailscale",
    value: "Tailscale",
    detail: "el servicio de coordinación y la flota DERP los opera Tailscale",
  },
  {
    name: "ZeroTier",
    value: "ZeroTier Inc.",
    detail:
      "los cuatro roots los opera ZeroTier Inc.; el controller sí se autoaloja",
  },
];

export const COLUMNS = ["Atenea One", "Tailscale", "ZeroTier"];

export const GROUPS = [
  {
    title: "criptografía del plano de datos",
    rows: [
      {
        topic: "construcción",
        cells: [
          [
            "Noise_IK_25519_ChaChaPoly_BLAKE2s, sobre la biblioteca snow; sin primitivas propias",
          ],
          ["WireGuard: Noise_IKpsk2_25519_ChaChaPoly_BLAKE2s", "wg"],
          [
            "protocolo original (VL1): Curve25519/Ed25519 con Salsa20 de 256 bits y Poly1305, encrypt-then-MAC como NaCl",
            "ztProto",
          ],
        ],
      },
      {
        topic: "secreto hacia adelante",
        cells: [
          ["sí: el rekey es un handshake nuevo con claves efímeras"],
          ["sí: el handshake se repite cada pocos minutos", "wg"],
          [
            "no en VL1; recomiendan TLS o SSH por encima si se necesita",
            "ztProto",
          ],
        ],
      },
      {
        topic: "rotación de claves de sesión",
        cells: [
          ["rekey suave a 120 s o 2^32 mensajes; rechazo a 180 s o 2^40"],
          ["por tiempo (REKEY_AFTER_TIME) y por número de mensajes", "wg"],
          ["sin estado criptográfico que rotar en VL1", "ztProto"],
        ],
      },
      {
        topic: "antirreplay",
        cells: [
          [
            "contador de 64 bits y ventana deslizante de 2048, actualizada solo tras autenticar",
          ],
          ["contador de 64 bits y ventana de unos 2000", "wg"],
          ["—"],
        ],
      },
      {
        topic: "PSK / poscuántico",
        cells: [
          ["sin PSK; poscuántico fuera de alcance"],
          ["PSK opcional en WireGuard", "wg"],
          ["—"],
        ],
      },
      {
        topic: "anti-DoS del handshake",
        cells: [
          [
            "MAC1 siempre, MAC2 con cookie bajo carga; respuesta de 62 bytes a un INIT de 146: no amplifica",
          ],
          [
            "MAC1 y MAC2 con cookie bajo carga; silencio ante mensajes no autenticados",
            "wg",
          ],
          ["—"],
        ],
      },
      {
        topic: "cifrado desactivable",
        cells: [
          ["no"],
          ["—"],
          [
            "sí: los trusted paths omiten cifrado y autenticación en un camino físico designado",
            "ztProto",
          ],
        ],
      },
      {
        topic: "sobrecarga y MTU",
        cells: [
          [
            "30 bytes por paquete de datos; MTU por defecto 1400; sin compresión ni relleno",
          ],
          ["—"],
          ["—"],
        ],
      },
    ],
  },
  {
    title: "conectividad y travesía de NAT",
    rows: [
      {
        topic: "primer paquete",
        cells: [
          ["espera a tener camino: directo por rendezvous o, si no, relay"],
          [
            "toda conexión empieza relayada por DERP y luego intenta subir a directa",
            "tsConn",
          ],
          [
            "sale de inmediato hacia arriba por un root, que además dispara el intento directo",
            "ztProto",
          ],
        ],
      },
      {
        topic: "señalización",
        cells: [
          [
            "coordinator propio: anuncios firmados y rendezvous; da pistas sin autenticar y no recibe claves",
          ],
          [
            "servicio de coordinación y DERP, con un protocolo de descubrimiento UDP propio cifrado con NaCl box",
            "tsCrypto",
          ],
          [
            "mensajes rendezvous de los roots, organizados como el DNS",
            "ztProto",
          ],
        ],
      },
      {
        topic: "técnicas",
        cells: [
          [
            "STUN propio y hole punching; candidatos acotados y saneados por par",
          ],
          [
            "varias estrategias de travesía; NAT-PMP y UPnP si el router los ofrece",
            "tsConn",
          ],
          [
            "hole punching, descubrimiento en LAN, predicción de puertos en NAT simétrico, uPnP y NAT-PMP",
            "ztProto",
          ],
        ],
      },
      {
        topic: "respaldo sin camino directo",
        cells: [
          ["relay UDP y, si UDP está bloqueado, relay TLS sobre TCP 443"],
          [
            "peer relay dentro de tu tailnet y, si no hay, DERP sobre HTTPS",
            "tsConn",
          ],
          ["relay por los roots, descrito como gratuito pero lento", "ztProto"],
        ],
      },
      {
        topic: "quién opera los relays",
        cells: [
          ["tú; no hay flota global"],
          [
            "Tailscale, en decenas de ciudades; DERP propio opcional y peer relays tuyos",
            "tsDerp",
          ],
          ["ZeroTier Inc.", "ztSelf"],
        ],
      },
      {
        topic: "qué ve el relay",
        cells: [
          ["datagramas cifrados; direcciones, tamaños y tiempos"],
          ["paquetes WireGuard ya cifrados", "tsDerp"],
          ["tráfico cifrado de extremo a extremo", "ztSelf"],
        ],
      },
      {
        topic: "cambio de camino",
        cells: [
          [
            "misma sesión y mismos contadores en cualquier camino; histéresis de 5 s al volver al mejor",
          ],
          [
            "reevalúa periódicamente si puede subir a directa o a peer relay",
            "tsConn",
          ],
          [
            "reintenta la conexión directa periódicamente, sin rendirse",
            "ztProto",
          ],
        ],
      },
    ],
  },
  {
    title: "identidad, autorización y confianza",
    rows: [
      {
        topic: "quién autoriza un dispositivo",
        cells: [
          [
            "configuración firmada por un emisor que certifica una raíz fuera de línea",
          ],
          [
            "el servicio de coordinación reparte las claves públicas de nodo",
            "tsLock",
          ],
          [
            "el controller de la red, que actúa como autoridad de certificación",
            "ztProto",
          ],
        ],
      },
      {
        topic: "si se compromete esa pieza",
        cells: [
          [
            "el emisor puede autorizar identidades hasta que caduque su certificado; la raíz no está en el servidor",
          ],
          [
            "sin Tailnet Lock podría insertar nodos; con Tailnet Lock (opcional, TOFU, firmas Ed25519) no",
            "tsLock",
          ],
          [
            "con la clave secreta del controller se emiten configuraciones fraudulentas o se admiten miembros",
            "ztProto",
          ],
        ],
      },
      {
        topic: "identidad del dispositivo",
        cells: [
          [
            "clave estática X25519 y clave de identidad, enlazadas en la configuración firmada",
          ],
          [
            "clave de nodo generada en el dispositivo; la privada no sale de él",
            "tsLock",
          ],
          [
            "dirección de 40 bits derivada de la clave pública; el alta es por orden de llegada",
            "ztProto",
          ],
        ],
      },
      {
        topic: "autenticación de personas",
        cells: [
          ["OIDC en el control plane, para administradores"],
          [
            "delegada en OAuth 2.0, OIDC o un proveedor de identidad",
            "tsControl",
          ],
          ["—"],
        ],
      },
      {
        topic: "revocación",
        cells: [
          [
            "revocar es dejar de emitir: ≤ 60 s en pares que alcanzan al emisor, ≤ 330 s en el peor caso",
          ],
          [
            "la gestiona la coordinación; si está caída no se puede revocar",
            "tsDown",
          ],
          ["—"],
        ],
      },
      {
        topic: "si cae el plano de control",
        cells: [
          [
            "los túneles caen al vencer el lease, en unos 5 minutos: se falla cerrado",
          ],
          [
            "lo establecido sigue hasta que caduquen las claves; no entran dispositivos ni cambian reglas",
            "tsDown",
          ],
          ["—"],
        ],
      },
    ],
  },
  {
    title: "política de acceso",
    rows: [
      {
        topic: "dónde se aplica",
        cells: [
          ["en el agente, en origen y en destino"],
          [
            "en cada dispositivo, con reglas calculadas por la coordinación y guardadas en caché",
            "tsDown",
          ],
          [
            "en el agente, en emisor y receptor; para saltarla hay que comprometer ambos lados",
            "ztRules",
          ],
        ],
      },
      {
        topic: "por defecto",
        cells: [
          ["denegación"],
          ["—"],
          ["reglas desactivadas: se permite todo el tráfico", "ztRules"],
        ],
      },
      {
        topic: "seguimiento de conexiones",
        cells: [
          [
            "no: sin estado; la regla inversa permite iniciar en sentido contrario sin restricción de puertos",
          ],
          ["—"],
          [
            "no: sin estado por diseño; hay que permitir los dos sentidos del flujo",
            "ztRules",
          ],
        ],
      },
      {
        topic: "expresividad",
        cells: [
          [
            "selectores de dispositivo y subred, protocolo y puertos; hasta 256 políticas por red",
          ],
          [
            "políticas de acceso, enrutado de subredes y exit nodes, entre otras",
            "tsControl",
          ],
          [
            "reglas L2 a L4 (EtherType, MAC, IP, puertos), etiquetas y capacidades firmadas; 1024 entradas en el conjunto base",
            "ztRules",
          ],
        ],
      },
      {
        topic: "acciones además de permitir o denegar",
        cells: [
          ["❌"],
          ["—"],
          [
            "TEE, WATCH y REDIRECT: copiar o redirigir tráfico a otro nodo",
            "ztRules",
          ],
        ],
      },
    ],
  },
  {
    title: "modelo de red y funciones",
    rows: [
      {
        topic: "capa",
        cells: [
          ["L3; el modo L2 está diseñado y en pausa hasta la auditoría"],
          ["túneles WireGuard entre dispositivos", "tsControl"],
          [
            "Ethernet virtual (VL2), similar a VXLAN, con multidifusión",
            "ztProto",
          ],
        ],
      },
      {
        topic: "subredes",
        cells: [
          [
            "router Linux, rutas aprobadas por un administrador, solo prefijos privados, SNAT explícito",
          ],
          ["subnet routers", "tsControl"],
          ["—"],
        ],
      },
      {
        topic: "salida a Internet (exit node)",
        cells: [
          ["no, por diseño: nunca /0 ni ruta por defecto"],
          ["sí", "tsControl"],
          ["—"],
        ],
      },
      {
        topic: "DNS",
        cells: [
          ["DNS dividido por enlace, firmado en la configuración"],
          ["lo distribuye la coordinación; MagicDNS", "tsControl"],
          ["—"],
        ],
      },
      {
        topic: "clientes de escritorio",
        cells: [
          ["Linux, macOS y Windows; aplicación de escritorio en los tres"],
          [
            "Linux, Windows y macOS; la GUI es cerrada en los sistemas cerrados",
            "tsOss",
          ],
          ["—"],
        ],
      },
      {
        topic: "clientes móviles",
        cells: [
          ["no hay: ni Android ni iOS"],
          ["Android, con cliente de código abierto, e iOS", "tsOss", "tsLock"],
          [
            "Android e iOS, gratuitas en Google Play y en la App Store",
            "ztProto",
          ],
        ],
      },
      {
        topic: "escala por red",
        cells: [["256 dispositivos aprobados por red"], ["—"], ["—"]],
      },
    ],
  },
  {
    title: "operación, código y madurez",
    rows: [
      {
        topic: "autoalojar el plano de control",
        cells: [
          ["sí, entero: control plane, coordinator, STUN y relays"],
          [
            "no el oficial, que es cerrado; Headscale es una alternativa comunitaria independiente",
            "tsOss",
          ],
          [
            "controller sí, sin interfaz web; los roots, como contrato enterprise",
            "ztSelf",
          ],
        ],
      },
      {
        topic: "código abierto",
        cells: [
          ["todo, bajo Apache-2.0"],
          [
            "cliente y DERP abiertos; las GUI de sistemas cerrados y el servidor de coordinación, cerrados",
            "tsOss",
          ],
          ["el controller es de código abierto", "ztSelf"],
        ],
      },
      {
        topic: "alta disponibilidad",
        cells: [
          ["no: sin HA del control plane, el coordinator ni la base de datos"],
          [
            "la mayoría de regiones DERP tienen al menos tres servidores",
            "tsDerp",
          ],
          [
            "cuatro roots repartidos por el mundo y por varios proveedores",
            "ztProto",
          ],
        ],
      },
      {
        topic: "revisión externa del protocolo",
        cells: [
          ["pendiente: no hay auditoría independiente"],
          [
            "declaran sus implementaciones revisadas por criptógrafos",
            "tsCrypto",
          ],
          ["—"],
        ],
      },
      {
        topic: "benchmark cara a cara",
        cells: [["❌"], ["❌"], ["❌"]],
      },
    ],
  },
];

/** Para qué sirve mejor cada uno, dicho sin barrer para casa. */
export const FIT = [
  {
    name: "Atenea One",
    highlight: true,
    points: [
      "Quieres que ninguna pieza de coordinación ni de relay dependa de un tercero.",
      "Prefieres que la red falle cerrada y que la revocación tenga una cota conocida.",
      "Aceptas operar tú la infraestructura y un producto aún sin auditar.",
    ],
  },
  {
    name: "Tailscale",
    points: [
      "Quieres no operar nada: coordinación y relays gestionados y en muchas regiones.",
      "Necesitas exit nodes, clientes móviles o un protocolo de datos ya muy revisado.",
      "Aceptas confiar en su coordinación, o activas Tailnet Lock.",
    ],
  },
  {
    name: "ZeroTier",
    points: [
      "Necesitas una red Ethernet virtual: L2, multidifusión, reglas por EtherType o MAC.",
      "Quieres conectividad inmediata, aunque empiece relayada.",
      "No necesitas secreto hacia adelante en la capa de transporte.",
    ],
  },
];

export const NOTES = [
  "Esto compara cómo está construido cada sistema, no cuánto rinde. No hemos medido Tailscale ni ZeroTier: ninguna cifra de esta web los incluye, y las de rendimiento de Atenea One son de laboratorio.",
  "Una raya (—) significa que no encontramos el dato en las fuentes consultadas, no que el producto carezca de esa función. La cruz sí marca una ausencia.",
  "Atenea One usa la misma familia de construcción que WireGuard (Noise IK con X25519, ChaCha20-Poly1305 y BLAKE2s), pero el cable, el antirreplay y la gestión de sesiones son código propio sin revisión externa. Esa es la diferencia que más pesa en contra.",
  "Fallar cerrado tiene un coste: si el emisor de Atenea One está caído más que la vigencia del lease, todos los túneles caen. En Tailscale, lo ya establecido sigue funcionando mientras no caduquen las claves.",
  "El proyecto no promete equivalencia con ZeroTier ni con Tailscale.",
];

/**
 * La tabla corta de la portada. Cada celda es un estado (`yes`, `no`, `wip` o
 * `na`) y sale de una fila de `GROUPS` o de un ADR: ninguna marca aquí contradice
 * a la tabla completa. `na` es «no encontrado en las fuentes», nunca «no».
 *
 * @type {{ topic: string; hint?: string; cells: ("yes" | "no" | "wip" | "na")[] }[]}
 */
export const SUMMARY = [
  {
    topic: "Coordinación y rendezvous sin terceros",
    hint: "Rendezvous: el paso en el que dos dispositivos detrás de NAT se encuentran. Un tercero con IP pública les pasa las direcciones candidatas del otro y los dos envían paquetes a la vez para abrir un camino directo. En Atenea One lo hace el coordinator, que solo da pistas sin autenticar: el túnel lo crea únicamente un handshake válido.",
    cells: ["yes", "no", "no"],
  },
  {
    topic: "Código abierto: cliente y plano de control",
    cells: ["yes", "no", "na"],
  },
  {
    topic: "Relays autoalojables: UDP y TLS/TCP 443",
    cells: ["yes", "yes", "na"],
  },
  {
    topic: "Travesía de NAT: STUN y hole punching",
    hint: "Hole punching: los dos dispositivos se envían paquetes UDP a la vez, de modo que el NAT de cada uno abre una entrada que deja pasar la respuesta. STUN les dice qué dirección y puerto públicos ven desde fuera.",
    cells: ["yes", "yes", "yes"],
  },
  {
    topic: "Cifrado de extremo a extremo con relay opaco",
    hint: "Relay opaco: solo reenvía datagramas ya cifrados entre los dos extremos. Ve direcciones IP, tamaños y tiempos, pero no el contenido.",
    cells: ["yes", "yes", "yes"],
  },
  {
    topic: "PFS: claves de sesión efímeras en el handshake",
    hint: "PFS (secreto perfecto hacia adelante): cada sesión usa claves efímeras que se descartan. Si luego se roba una clave de larga duración, no sirve para descifrar el tráfico ya capturado.",
    cells: ["yes", "yes", "no"],
  },
  {
    topic: "Política de acceso default-deny",
    hint: "Default-deny: sin una regla que lo permita, el tráfico se descarta. Lo contrario es permitirlo todo hasta que se añaden reglas.",
    cells: ["yes", "na", "no"],
  },
  {
    topic: "Aplicación de la política en el endpoint",
    hint: "Endpoint: el propio dispositivo. El filtrado ocurre en el agente de cada extremo, no en un servidor intermedio.",
    cells: ["yes", "yes", "yes"],
  },
  {
    topic: "Plano de datos activo con el plano de control caído",
    hint: "El plano de control reparte claves, configuración y política; el plano de datos es el túnel por el que va el tráfico. Atenea One falla cerrado: si el emisor cae más que la vigencia del lease (unos 5 minutos), los túneles caen.",
    cells: ["no", "yes", "na"],
  },
  {
    topic: "Auditoría criptográfica externa del protocolo",
    cells: ["wip", "na", "na"],
  },
  {
    topic: "Capa 2: Ethernet virtual (L2)",
    hint: "Capa 2: la red se comporta como un cable Ethernet, con difusión y multidifusión. Las redes de Atenea One son de capa 3 (IP).",
    cells: ["wip", "na", "yes"],
  },
  {
    topic: "Exit node: ruta por defecto vía un par",
    hint: "Exit node: un dispositivo por el que sale a Internet todo el tráfico de otro (ruta por defecto 0.0.0.0/0). Atenea One no lo permite, por diseño.",
    cells: ["no", "yes", "na"],
  },
  {
    topic: "Clientes de escritorio: Linux, macOS y Windows",
    cells: ["yes", "yes", "na"],
  },
  {
    topic: "Autenticación de administradores: OIDC",
    hint: "OIDC (OpenID Connect): el estándar para iniciar sesión con un proveedor de identidad externo, como Google, Microsoft u Okta.",
    cells: ["yes", "yes", "na"],
  },
  {
    topic: "Subnet router: rutas hacia una LAN",
    hint: "Subnet router: un dispositivo que da al resto de la red acceso a una red local entera, sin instalar el cliente en cada equipo de esa red.",
    cells: ["yes", "yes", "na"],
  },
  {
    topic: "DNS interno: resolución por nombre de host",
    cells: ["yes", "yes", "na"],
  },
  {
    topic: "Clientes móviles: Android e iOS",
    cells: ["no", "yes", "yes"],
  },
];

/** @type {{ status: "yes" | "no" | "wip" | "na"; text: string }[]} */
export const LEGEND = [
  { status: "yes", text: "lo tiene" },
  { status: "no", text: "no lo tiene" },
  { status: "wip", text: "en curso o pendiente" },
  { status: "na", text: "sin dato en las fuentes" },
];
