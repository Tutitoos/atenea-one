# Relay v2 y conmutación de caminos

- **Estado:** evolución incompatible aceptada por el responsable el 2026-10-25; contrato normativo pendiente de implementación, vectores y revisión externa. Sustituye íntegramente [relay-v1](relay-v1.md).
- **Relacionados:** [wire-v2](wire-v2.md), ADR-001/003/004, [coordinator-v1](coordinator-v1.md), modelo de amenazas T18–T22.
- Enteros sin signo big-endian, tiempos Unix en segundos, `||` concatenación; dominios ASCII incluyen `\0` únicamente donde se indica. Parsers estrictos: tamaños exactos, sin sobrantes y booleanos exclusivamente 0/1.

## 1. Principios y compatibilidad

El relay transporta datagramas Atenea v2 opacos, sin claves E2E. Solo reenvía entre asignaciones emparejadas `(network,X,Y)` y `(network,Y,X)`; nunca hacia una dirección elegida dentro del mensaje por el cliente. Cookie UDP o conexión TLS validan el origen antes de asignar; token del coordinator y prueba Ed25519 autorizan la asignación. El relay ve metadatos y puede descartar/retrasar; Noise y la ventana del destinatario protegen integridad y replay, no disponibilidad.

**No hay compatibilidad con relay v1 ni wire v1.** Todo mensaje v2 comienza `magic=0xA1 || version=0x02 || type`. La versión ocupa el byte 1 y el tipo el byte 2. En v1 no había versión separada: su segundo byte era el tipo, por lo que no se debe deducir compatibilidad de un byte 0x02. El antiguo COOKIE v1 (`a1 02` + 16 bytes) no tiene longitud/tipo v2 válidos y se rechaza. DATA solo lleva wire-v2 válido estructuralmente; el relay no descifra ni comprueba MAC1/Noise.

Versión desconocida, v1 o cuerpo inválido por UDP: **descarte silencioso**. Un datagrama E2E de versión desconocida se descarta también dentro de DATA. En TCP: ALPN exclusivo `atenea-relay/2`; cliente v1 recibe error TLS de negociación ALPN (sin anunciar `/1`). Una conexión con ALPN `/2` que presente cabecera de versión incorrecta recibe una única trama v2 `ALLOC_ERR(UnsupportedVersion=10)` y se cierra; cuerpo malformado (incluido v1 ambiguo) recibe `Malformed=11` y cierre. No se interpreta/ejecuta ningún mensaje v1 para responderle. El cliente informa «relay incompatible: se requiere relay-v2» para ALPN/UnsupportedVersion y «mensaje de relay inválido» para Malformed. Los errores UDP solo se devuelven para ALLOC v2 de estructura válida con cookie vigente; nunca se responde a una versión desconocida.

## 2. Identidad del relay y configuración

`RelayId = [u8;16]`: identificador aleatorio de 128 bits generado una vez por el operador con RNG del SO, no secreto, no derivado de DNS/IP/certificado ni de una clave del coordinator. En JSON/TOML se codifica como exactamente 32 caracteres hex **minúsculos**; se rechaza todo-cero. Persistirlo en la configuración de esa instancia, independiente de la rotación de certificados/claves. Relays distintos deben tener IDs distintos. No es una prueba criptográfica de identidad del servidor: TLS autentica el nombre y el token fija la audiencia; un relay hostil no obtiene la clave del dispositivo por conocer el ID.

- Relay: `ATENEA_RELAY_ID` / `relay_id` en TOML, obligatorio incluso en laboratorio; sin valor o mal formado no arranca.
- Coordinator: cada entrada de `ATENEA_COORD_RELAYS` / `relays` incluye `relay_id`, `name`, y `udp` y/o `tls` (`tls_name` obligatorio si ofrece TLS). Entre 1 y **4** entradas al habilitar relay; IDs duplicados, nombres vacíos o nombres duplicados son error de configuración al arrancar; sin signer o lista vacía, `/v1/relay` devuelve 503. `name` es descriptivo, no una identidad firmada. Coordinator y relay reciben el mismo ID por gestión del operador; no hay descubrimiento que lo cambie.
- Un cambio de relay_id requiere actualizar ambos lados y emitir tokens nuevos. No se conserva un alias del ID antiguo. Dos procesos con el mismo ID son una única audiencia de seguridad y no ofrecen aislamiento entre sí: no configurarlos así si se necesita separación.

## 3. Mensajes UDP y cuerpos TCP

Todos incluyen cabecera de 3 bytes `a1 02 type`; las longitudes de la tabla incluyen esa cabecera, no el prefijo TCP.

| type | Campos después de la cabecera | Longitud total |
| --- | --- | --- |
| 01 COOKIE_REQ | relay_id[16], padding[45] todo cero | **64** |
| 02 COOKIE | relay_id[16], cookie[16] | **35** |
| 03 ALLOC | relay_id[16], cookie[16], ts u64, token_len u16 = 189, token[189], sig[64] | **298** |
| 04 ALLOC_OK | alloc_id u64, ttl_secs u16, peer_ready u8 | **14** |
| 05 ALLOC_ERR | code u8 | **4** |
| 06 DATA | alloc_id u64, datagrama wire-v2[30..1530] o INIT/RESP/COOKIE_REPLY con su longitud | **11 + datagrama** (máximo 1541) |
| 08 PING / 09 PONG | alloc_id u64, nonce u64 | **19** |
| 0a PEER_STATE | alloc_id u64, ready u8 | **12** |

COOKIE_REQ para otro ID se descarta, sin respuesta. El cliente exige `COOKIE.relay_id` igual al relay solicitado y origen igual al endpoint contactado; una COOKIE no autentica al servidor. Cookie de **asignación del relay**, distinta de la de INIT E2E de wire-v2:

```
relay_cookie = BLAKE2s-256(secret[32] || relay_id[16] || ip || port u16 || floor(unix_time/60) u64)[0..16]
```

Se conserva la construcción v1 de cookie del relay añadiendo audiencia: `ip` = IPv4[4] o IPv6[16], normalizando mapped IPv4; las longitudes desambiguan familia. El secreto es aleatorio por proceso y no compartido entre IDs. Se aceptan minuto vigente/anterior, sin underflow al calcular el anterior. COOKIE_REQ ≤ 20/s por IPv4 o IPv6 /64 y tabla acotada; respuesta 35 ≤ petición 64. COOKIE_REQ/COOKIE están prohibidos en TCP. **Esta cookie no es la keyed-BLAKE2s de wire-v2**, ni se comparte su secreto.

Tramado TCP: longitud u16 big-endian y cuerpo; longitud 0 o >2048 cierra, lecturas parciales soportadas, memoria acotada. Antes de asignarse solo ALLOC; cookie TCP debe ser 16 ceros. Se exige TLS 1.3, certificado/nombre verificado, ALPN `/2`, sin 0-RTT. Tiempo TLS 10 s, primer ALLOC 5 s, inactividad 90 s. PROXY v2 solo desde loopback.

## 4. Token y prueba de posesión

**TOKEN_LEN = 189**; firma Ed25519 estricta. Todos los campos, incluido relay_id, quedan firmados:

| Offset | Campo | Bytes |
| --- | --- | --- |
| 0 | magic ASCII AORT | 4 |
| 4 | version = 2 | 1 |
| 5 | relay_id | 16 |
| 21 | network_id | 16 |
| 37 | src_device | 16 |
| 53 | dst_device | 16 |
| 69 | src_identity_pub (Ed25519) | 32 |
| 101 | not_before | 8 |
| 109 | not_after | 8 |
| 117 | token_id aleatorio | 8 |
| 125 | signature del coordinator | 64 |

`signature = Ed25519(coord_sk, "atenea-one/relay-token/v2\0" || token[0..125])`. Token v1, cualquier otra versión, tamaño incorrecto, firma inválida o IDs estructuralmente inválidos se rechazan. Debe cumplirse `src != dst`, `not_after > not_before`, vida ≤600 s y `not_before−30 ≤ now ≤ not_after+30`, con aritmética comprobada/saturada. El coordinator emite por defecto 300 s. Los IDs de red/dispositivo son los de la configuración firmada; `src_identity_pub` procede de su presencia vigente.

Entrada firmada del dispositivo (sin serializaciones JSON ni truncamiento):

```
sig = Ed25519(src_identity_sk,
  "atenea-one/relay-alloc/v2\0" || relay_id[16] || cookie[16] || ts u64 || token[189])
```

El relay exige **ALLOC.relay_id == token.relay_id == config.relay_id**. Se firma el ID explícitamente aunque ya esté dentro del token: ninguna API acepta una audiencia implícita. `|now−ts| ≤30 s`; renovar genera ts/firma nuevos. El ID sigue presente y firmado cuando cookie=0 en TCP. Un token o firma para otro relay no se adapta cambiando solo el encabezado/endpoint; se rechaza antes de crear/refrescar/mover una asignación.

### Validaciones y errores

Orden: versión/tamaño/tipo; cookie UDP (o cookie TCP cero); límite por origen; parseo del token; coincidencia de los tres IDs; firma del coordinator con clave permitida; tiempo token y ts; prueba Ed25519 del dispositivo; capacidad y autorización estructural. Cookie inválida UDP: silencio, sin firma ni recursos. Tras cookie válida/TLS, error de audiencia devuelve **9 WrongRelay** (aunque las firmas no se hayan comprobado, no cambia estado); el resto no crea ni refresca asignaciones.

| Código | Nombre | Condición |
| --- | --- | --- |
| 1 | BadToken | Token mal formado, versión incompatible o firma de coordinator inválida |
| 2 | Expired | Token o ts fuera de ventana, o vida inválida |
| 3 | RateLimited | Cuota de tasa |
| 4 | Full | Capacidad agotada |
| 5 | NotFound | Asignación inexistente en una operación que admita error |
| 6 | BadSignature | Prueba del dispositivo inválida |
| 7 | BadCookie | Cookie TCP no cero; UDP inválida se descarta |
| 8 | Unauthorized | Identidad/par no autorizado o src == dst |
| 9 | WrongRelay | Audiencia ALLOC/token distinta de este relay |
| 10 | UnsupportedVersion | Versión de cuerpo relay incompatible, solo TCP antes de cierre |
| 11 | Malformed | Cuerpo relay mal formado, solo TCP antes de cierre |

Una firma ALLOC sigue siendo repetible **en el mismo relay** dentro de ±30 s y el token puede renovar durante su vigencia: no se añade caché de token_id/firmas. UDP exige cookie ligada al origen y DATA exige la vinculación; TCP valida conexión, no un desafío exclusivo por conexión. Por tanto la audiencia elimina reutilización **entre IDs distintos**, no toda repetición dentro de la misma audiencia. Sin 0-RTT. Registrar motivos/códigos, nunca tokens, cookies, firmas ni payload.

## 5. Estado, reenvío y cuotas (conservados)

ALLOC válido crea o refresca `(network,src,dst)` con alloc_id aleatorio impredecible y vincula dirección UDP exacta o conexión TCP (más reciente gana). Vida = `min(token.not_after+30−now,120)` segundos, calculada sin wrap; TTL positivo. Renovación cada 60 s con token vigente; barrido cada 5 s. DATA/PING solo de la vinculación vigente. DATA de A solo sale a B=(net,dst,src), con alloc_id de B; sin pareja, silencio. PEER_STATE avisa aparición/desaparición de la pareja. Nunca se aprende la IP de destino de la carga E2E. Los límites y la vida se aplican por relay local, no globalmente entre instancias.

Valores por defecto configurables: 4096 asignaciones, 1024/red, 32/origen (IPv4 o IPv6 /64), 2000 paquetes/s y 24 Mbit/s por asignación, ALLOC 10/s/origen, COOKIE_REQ 20/s/origen, DATA E2E ≤1530; cola TCP de 256 datagramas (descartar nuevo al llenarse), 8 conexiones/origen, 2048 totales. Tablas de tasas acotadas y recuperación de entradas completamente repuestas al llenarse; barridos de reposición no más de cada 100 ms. `max_per_ip` admite 1..=MAX_ALLOCATIONS (≤1.000.000); malla N consume N·(N−1) asignaciones, K agentes tras una IP consumen K·(N−1). Conservar ajuste operativo y métricas existentes, actualizando sobrecarga DATA a 11.

## 6. Emisión `/v1/relay` y cliente

HTTP conserva `/v1` y AtDevice v1; request firmado `{"peer":"<hex16>"}` y autorización mutua/presencias igual que rendezvous. La lista de relays no lleva token compartido. Respuesta exacta:

```json
{"relays":[{"relay_id":"11111111111111111111111111111111","name":"relay-a","udp":"relay.example:443","tls":"relay.example:443","tls_name":"relay.example","token":"<base64 de 189 bytes para solicitante→par EN ESTE relay>"}],"ttl_secs":300}
```

`udp`/`tls` opcionales según configuración; token obligatorio en **cada** entrada. No hay `token` en el nivel raíz. El aviso inverso usa `{"kind":"relay","peer":"<solicitante>","relays":[…],"ttl_secs":300,"seq":N}` con tokens distintos para `par→solicitante`, cada uno ligado al mismo relay_id de su entrada. Un token_id aleatorio por token. Mantener orden configurado. Solo una lista de máximo 4; no truncar silenciosamente una configuración mayor.

Se conservan respuestas uniformes: 401 invalid_signature, 409 not_announced, 404 peer_unavailable, 429 rate_limited y 503 relay_unavailable. Configuración de relay_id inválida impide arrancar, no un 500 en emisión. Límites: ≤1 emisión por 5 s/par, ≤2 emisiones por minuto/par y ≤10 solicitudes/min/dispositivo. Una emisión genera **2×R tokens** para R relays; el límite antiguo de cuatro tokens/min se redefine a **cuatro concesiones direccionales/min** (= dos emisiones HTTP), cada concesión con ≤4 tokens de audiencia. Máximo 16 tokens reales/min/par. No reducir aleatoriamente relays para encajar el contador antiguo. Eventos en cola de 16, TTL 30 s, como antes.

Cliente: `open(peer, grants)` donde cada grant contiene endpoint+relay_id+token, sin token global. Verifica estructura v2 y coherencia de ID/src/dst/red antes de conectar (esto no sustituye la firma que verifica el relay); rechaza concesión antigua/incompleta con error local de protocolo, sin intentar v1. Conservar saneado de ≤4 destinos, DNS con plazo y conexión a la IP verificada, excluir loopback/link-local/multicast/redes del túnel y subredes servidas. Por relay candidato, UDP primero (cookie→ALLOC, 1 s, hasta tres intentos), luego TLS si está disponible, usando **su propio token**; renovar cada 60 s, volver a pedir concesión al caducar. Cambiar de relay exige el token de ese ID, nunca reciclar el anterior. PING cada 5 s. Eventos Rx deben incluir `relay_id` además de peer, transporte y bytes para la cookie E2E de wire-v2; metadatos tomados de la asignación local, no de bytes del peer.

**Nota operativa del agente (aplicado el 2026-10-26, sin cambio del contrato
de cable):** obtiene concesiones anticipadamente aun con el relay listo
([plazos y backoff](../../apps/agent/README.md#transporte-estado-y-recuperación)).
El `upsert` implementado conserva el `alloc_id` para la misma clave
`(network,src,dst)`; refrescar con otro token válido del mismo relay y la misma
vinculación no requiere dos filas ni reiniciar el worker. El cliente distingue
la credencial confirmada de la pendiente: solo ALLOC_OK instala esta última.
Un rechazo o pérdida conserva la autorización previa únicamente hasta el menor
deadline vigente de token/asignación; el cliente corta en `not_after`, sin usar
la tolerancia de 30 s del servidor. Retirar la audiencia sigue cerrando la
asignación. Cada ALLOC conserva cookie, timestamp y prueba de posesión nuevos;
la renovación no autoriza reutilizar un token entre IDs ni promete solapamiento
entre relays o bindings distintos.

## 7. Caminos y despliegue

Conservar preferencia directo > relay UDP > relay TLS; un camino solo sano tras datos de sesión autenticados en últimos 9 s. Keepalive 4 s con alternativa, 15 s si único. Tras 4 s de tráfico sin retorno o sondeo directo sin éxito, solicitar relay (≤cada 5 s); al estar listo, enviar keepalive si hay sesión o INIT pendiente. Handshakes pueden viajar por directo y relay, pero **cada envío INIT aplica MAC2 de su contexto**, no reutiliza ciegamente el datagrama con cookie de otro camino; respuesta/cookie vuelve por el camino de entrada. Respuesta de cookie no cuenta como salud. Reintentos conservan ventana 10 s y presupuesto 12/candidato.

Conmutación a alternativa sana inmediata al morir el activo; subida a camino preferido tras 5 s sanos continuos. Sondeo directo cada 20 s con backoff; liberar relay 30 s después de que directo lleve 30 s estable. Mismo Session/contador para todas las rutas, sin rekey por cambiar camino; copias deduplicadas. Mantener estado IPC y métricas de caminos/asignaciones; añadir relay_id visible, sin secretos.

HAProxy ADR-004: actualizar regla ALPN a `atenea-relay/2`, SNI hacia el relay y PROXY v2 loopback. No prometer soporte de proxies HTTP/inspección TLS; TCP sobre TCP sigue siendo último recurso. No hay despliegues existentes: actualizar todos los binarios, fixtures y configuración juntos, no añadir adaptadores v1. El agente no puede emitir un error de red a un peer v1 (silencio obligatorio); informa localmente timeout/incompatibilidad si el contexto lo permite, sin afirmar versión remota no observada.

## 8. Impacto exacto e interfaces de implementación

Lista basada en el árbol actual; no pide cambios de política ACL, formato firmado de identidad ni parser IP interno. Primero implementar formatos y crypto, después consumidores; no cambiar primitivas Noise ni sacar claves de snow.

| Orden | Archivos | Contrato / cambios y pruebas exigidos |
| --- | --- | --- |
| 1 | `crates/protocol/src/lib.rs` | VERSION=2, INIT146, COOKIE_REPLY62, campos MAC separados sin copiar Noise, encoder/parser estricto; tests de round-trip, versiones/longitudes y mutaciones |
| 2 | `crates/crypto/src/lib.rs`, `crates/crypto/Cargo.toml` | prólogo v2, MAC keyed-16, secretos/rotación y cifrado de cookie; API recibe origen canónico y bytes exactos antes de DH; no criptografía ad hoc. Tests/vectores wire-v2 §7. `Cargo.lock` solo si Cargo lo necesita para dependencias directas **ya fijadas**, sin resolver versiones nuevas |
| 3 | `crates/relay-core/src/lib.rs` | RelayId16, header/version, token189, ALLOC298, dominios v2, errores9–11, cookies con ID, DATA_OVERHEAD11, ALPN; llamadas issue/verify_alloc reciben relay_id explícito, rechazan v1; tests de audiencia y firma/cookie/token mutados |
| 4 | `apps/relay/src/{config,state,tcp,lib,alloc}.rs`, `apps/relay/tests/relay.rs`, `apps/relay/README.md` | ID obligatorio y validaciones, orden/errores/cierres, ALPN, offsets/counters, token/perfil fixtures en alloc; sockets UDP/TLS, dos IDs con misma clave de coordinator: token y prueba A no asignan B; repetición misma audiencia documentada |
| 5 | `apps/coordinator/src/{config,lib,presence}.rs`, `apps/coordinator/tests/api.rs`, `apps/coordinator/README.md` | lista validada ≤4 IDs únicos; emisión 2R tokens/evento, JSON sin token raíz, cuotas de concesiones; tests 2 relays, 4 relays, duplicados, errores de config y autorización, límites sin multiplicar peticiones |
| 6 | `apps/agent/src/{config,coord,relay,engine,run}.rs`, `apps/agent/README.md` | opción de carga, grants por relay, transporte Rx/Out con relay_id, contexto cookie, caché por camino, reintento sin ampliar sondeo, orden antes de DH; `run` propaga metadatos, `coord` deserializa nuevo JSON; tests dentro de esos módulos y sus helpers `apps/agent/src/testutil.rs` |
| 7 | `crates/testpeer/src/peer.rs`, `crates/testpeer/src/peer/tests.rs`, `crates/testpeer/README.md` | INIT/MAC/cookie y prólogo v2, validación pre-DH; adaptar `receive` y fixtures de handshakes reales y mantener confirmación/antirreplay. `src/main.rs` consume ese módulo y solo cambia si necesita exponer configuración de carga |
| 8 | `fuzz/fuzz_targets/{handshake_init,datagram_parse,relay_parse,session_decrypt,agent_engine}.rs`, `fuzz/README.md` | cubrir envoltorio+MAC y límites pre-DH además de `read_init`, rama COOKIE_REPLY, round-trip MAC/nonce, token189/header v2 y tramas partidas; session_decrypt conserva antirreplay; agent_engine cubre desafío/rutas y estado acotado |
| 9 | `tests/network-lab/multi-peer.py`, `tests/network-lab/nat/{e2e-relay.sh,haproxy.cfg.tpl,haproxy-probe.py,README.md}`, `tests/stack/probe.py`, `tests/stack/drills/compose.yaml` | IDs en cada relay/concesión, dos audiencias distintas, pruebas NAT/relay/conmutación y ALPN `/2`; escenarios directos `tests/network-lab/nat/e2e.sh` y `tests/network-lab/run.sh` son regresiones, no requieren cambios de contrato |
| 10 | `deploy/compose/prod/{bootstrap.py,haproxy.cfg}`, `infra/ansible/relay-compose/relay.toml.example`, `docs/architecture/ADR-004-entrada-443.md` | generación de un ID estable compartido coordinator/relay, ejemplos de ID obligatorio y regla ALPN `/2`; no regenerar IDs en cada arranque |

`apps/agent/src/packet.rs` solo analiza IP **interna**, por lo que queda intencionalmente sin cambios por esta evolución. `crates/crypto/src/replay.rs` y `hex.rs` no necesitan cambiar: sus invariantes y pruebas son regresiones. `apps/coordinator/src/auth.rs` no cambia AtDevice ni nonces HTTP. Configuraciones firmadas v1/v2 siguen aceptándose según sus propios contratos y los vectores de identidad existentes no se regeneran.

Criterios transversales: probar desconocidos/v1 tanto UDP (silencio) como TCP (ALPN/error y cierre); dos relays con mismo coordinator y IDs distintos; cambio de cookie/ts/token/ID invalida prueba; reloj±30 y límites de vida; UDP/TLS conmensurables; cuotas y no amplificación; sin sesiones/cupos DH consumidos por desafío inválido; roaming y confirmación no cambian. Regresiones de laboratorios fases 2/3/3B/4/5, NAT directo/relay/TLS, conmutación, HAProxy y tráfico íntegro sin payload en claro. Los laboratorios Linux/Docker se **describen**, no se ejecutan en este trabajo: aquí solo documentación, sin red/descargas/Docker/Rust. La implementación debe publicar hex antes de conformidad, además de tests calculables; nunca llamar «verificado» al contrato por sí solo.

## 9. Vectores de relay, riesgos y decisiones

Vector reproducible: semilla Ed25519 coordinator=`01`×32; dispositivo=`02`×32; relay_id=`11`×16; red=`07`×16; src=`22`×16; dst=`33`×16; `not_before=1700000000`, `not_after=1700000300`, token_id=`44`×8; src_identity_pub derivada de semilla dispositivo. ALLOC TCP: cookie=`00`×16 y ts=1700000001. ALLOC UDP: cookie=`55`×16, mismo ts. COOKIE_REQ usa padding cero; cookie relay con secret=`00..1f`, IP `192.0.2.1`, puerto51820 y now1700000000. Los hex se publican debajo. Cambiar relay_id a `12`×16 en servidor, token o ALLOC da WrongRelay o firma inválida y ninguna asignación; token v1 da BadToken.

### Hex reproducible (generado el 2026-10-25)

Calculado independientemente con Python `cryptography` (Ed25519/OpenSSL) y
`hashlib.blake2s`; la prueba Rust `published_vectors_match_independent_python`
compara los bytes usando dalek/blake2. Esa prueba queda pendiente de ejecución
con el resto de la integración; generar los hex no sustituye revisión externa.
Cada bloque es una única secuencia hex sin espacios ni saltos.

| Salida | Hex |
| --- | --- |
| src_identity_pub | `8139770ea87d175f56a35466c34c7ecccb8d8a91b4ee37a25df60f5b8fc9b394` |
| cuerpo125 | `414f525402111111111111111111111111111111110707070707070707070707070707070722222222222222222222222222222222333333333333333333333333333333338139770ea87d175f56a35466c34c7ecccb8d8a91b4ee37a25df60f5b8fc9b394000000006553f100000000006553f22c4444444444444444` |
| firma token64 | `184d0e6f548da223384d684f37130a0a2c71e24be464c0e600c53b0bf22c26a5da740e636b609e100c11c05cd388acb5ec700c2e3560b9b75a18ece7dbb03f05` |
| token189 | `414f525402111111111111111111111111111111110707070707070707070707070707070722222222222222222222222222222222333333333333333333333333333333338139770ea87d175f56a35466c34c7ecccb8d8a91b4ee37a25df60f5b8fc9b394000000006553f100000000006553f22c4444444444444444184d0e6f548da223384d684f37130a0a2c71e24be464c0e600c53b0bf22c26a5da740e636b609e100c11c05cd388acb5ec700c2e3560b9b75a18ece7dbb03f05` |
| entrada firmada ALLOC TCP | `6174656e65612d6f6e652f72656c61792d616c6c6f632f7632001111111111111111111111111111111100000000000000000000000000000000000000006553f101414f525402111111111111111111111111111111110707070707070707070707070707070722222222222222222222222222222222333333333333333333333333333333338139770ea87d175f56a35466c34c7ecccb8d8a91b4ee37a25df60f5b8fc9b394000000006553f100000000006553f22c4444444444444444184d0e6f548da223384d684f37130a0a2c71e24be464c0e600c53b0bf22c26a5da740e636b609e100c11c05cd388acb5ec700c2e3560b9b75a18ece7dbb03f05` |
| firma ALLOC TCP64 | `e44191b9b550966bf46d3ffd8cb049f74ae4de4a06ae882a51d6bc90b5c944a30cf43d02a76dcc017f73de4ad560625eda662a45cf8c89e487ba02566f43e50f` |
| ALLOC TCP298 | `a102031111111111111111111111111111111100000000000000000000000000000000000000006553f10100bd414f525402111111111111111111111111111111110707070707070707070707070707070722222222222222222222222222222222333333333333333333333333333333338139770ea87d175f56a35466c34c7ecccb8d8a91b4ee37a25df60f5b8fc9b394000000006553f100000000006553f22c4444444444444444184d0e6f548da223384d684f37130a0a2c71e24be464c0e600c53b0bf22c26a5da740e636b609e100c11c05cd388acb5ec700c2e3560b9b75a18ece7dbb03f05e44191b9b550966bf46d3ffd8cb049f74ae4de4a06ae882a51d6bc90b5c944a30cf43d02a76dcc017f73de4ad560625eda662a45cf8c89e487ba02566f43e50f` |
| entrada firmada ALLOC UDP | `6174656e65612d6f6e652f72656c61792d616c6c6f632f7632001111111111111111111111111111111155555555555555555555555555555555000000006553f101414f525402111111111111111111111111111111110707070707070707070707070707070722222222222222222222222222222222333333333333333333333333333333338139770ea87d175f56a35466c34c7ecccb8d8a91b4ee37a25df60f5b8fc9b394000000006553f100000000006553f22c4444444444444444184d0e6f548da223384d684f37130a0a2c71e24be464c0e600c53b0bf22c26a5da740e636b609e100c11c05cd388acb5ec700c2e3560b9b75a18ece7dbb03f05` |
| firma ALLOC UDP64 | `65b3b9e5025bd7a2b826f86db19e39465907f95a21a872d0107c575422e8ab2483e146051a7b8e64ad7e860c6b4726d79b57d7110383259d964f48842040ba00` |
| ALLOC UDP298 | `a102031111111111111111111111111111111155555555555555555555555555555555000000006553f10100bd414f525402111111111111111111111111111111110707070707070707070707070707070722222222222222222222222222222222333333333333333333333333333333338139770ea87d175f56a35466c34c7ecccb8d8a91b4ee37a25df60f5b8fc9b394000000006553f100000000006553f22c4444444444444444184d0e6f548da223384d684f37130a0a2c71e24be464c0e600c53b0bf22c26a5da740e636b609e100c11c05cd388acb5ec700c2e3560b9b75a18ece7dbb03f0565b3b9e5025bd7a2b826f86db19e39465907f95a21a872d0107c575422e8ab2483e146051a7b8e64ad7e860c6b4726d79b57d7110383259d964f48842040ba00` |
| relay_cookie16 | `592e6e98cedbe48bd9e2897c403f2b33` |
| COOKIE35 | `a1020211111111111111111111111111111111592e6e98cedbe48bd9e2897c403f2b33` |
| COOKIE_REQ64 | `a1020111111111111111111111111111111111000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000` |
| trama TCP300 (cookie cero) | `012aa102031111111111111111111111111111111100000000000000000000000000000000000000006553f10100bd414f525402111111111111111111111111111111110707070707070707070707070707070722222222222222222222222222222222333333333333333333333333333333338139770ea87d175f56a35466c34c7ecccb8d8a91b4ee37a25df60f5b8fc9b394000000006553f100000000006553f22c4444444444444444184d0e6f548da223384d684f37130a0a2c71e24be464c0e600c53b0bf22c26a5da740e636b609e100c11c05cd388acb5ec700c2e3560b9b75a18ece7dbb03f05e44191b9b550966bf46d3ffd8cb049f74ae4de4a06ae882a51d6bc90b5c944a30cf43d02a76dcc017f73de4ad560625eda662a45cf8c89e487ba02566f43e50f` |
| envoltura TCP300 del vector UDP (solo tramado; cookie no admitida por TCP) | `012aa102031111111111111111111111111111111155555555555555555555555555555555000000006553f10100bd414f525402111111111111111111111111111111110707070707070707070707070707070722222222222222222222222222222222333333333333333333333333333333338139770ea87d175f56a35466c34c7ecccb8d8a91b4ee37a25df60f5b8fc9b394000000006553f100000000006553f22c4444444444444444184d0e6f548da223384d684f37130a0a2c71e24be464c0e600c53b0bf22c26a5da740e636b609e100c11c05cd388acb5ec700c2e3560b9b75a18ece7dbb03f0565b3b9e5025bd7a2b826f86db19e39465907f95a21a872d0107c575422e8ab2483e146051a7b8e64ad7e860c6b4726d79b57d7110383259d964f48842040ba00` |

Las tramas TCP de ambos ALLOC son exactamente `012a || ALLOC298` (300 bytes);
no se permiten COOKIE_REQ/COOKIE en TCP. Reproducción: serializar los enteros con
`struct.pack(">QQ",1700000000,1700000300)` y `struct.pack(">Q",1700000001)`;
`Ed25519PrivateKey.from_private_bytes(bytes([1])*32)` firma el dominio de token
y cuerpo125; la clave de semilla `02` firma las entradas ALLOC publicadas.
La cookie es `hashlib.blake2s(bytes(range(32)) + bytes.fromhex("11"*16) +
bytes.fromhex("c0000201ca6c") + struct.pack(">Q",1700000000//60)).digest()[:16]`.

Errata de frontera: aunque el token sea válido exactamente en `not_after+30`,
un ALLOC cuya vida calculada sea cero devuelve `Expired` sin mutar asignaciones.
Los IDs de relay/red/dispositivo todo-cero son estructuralmente inválidos;
`src == dst` sigue devolviendo `Unauthorized` tras verificar las firmas.

No queda pregunta funcional abierta. Decisiones: ID explícito aleatorio evita inventar otra clave de relay/derivación y permanece estable tras rotación TLS; IDs distintos son un requisito operativo de aislamiento. Se conserva repetición dentro del mismo relay durante frescura, no se promete resistencia a toda retransmisión. No HA, balanceo, selección regional, QUIC, TURN, cuotas por organización ni ACK de cookies: fuera de este cambio. La revisión externa y mediciones de carga siguen siendo puertas previas a producción, no hechos consumados.
