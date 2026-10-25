# Cable v2: datagramas y cookie del mensaje 1

- **Estado:** decisión aceptada por el responsable el 2026-10-25; especificación normativa para implementar y auditar, no declaración de implementación ni de verificación.
- **Relacionados:** ADR-001, ADR-003, [relay-v2](relay-v2.md), modelo de amenazas T10/T30.
- «Debe» y «no debe» son requisitos. Enteros sin signo big-endian; intervalos de bytes `[a,b)`; concatenación `||`; textos ASCII literales sin terminador salvo indicación expresa.

## 1. Cutover y formatos

El primer byte de **todo** datagrama Atenea es `version=0x02`; el segundo es `type`. Se rechazan v1 y cualquier versión desconocida mediante descarte silencioso, también dentro de DATA de un relay. No hay negociación, fallback ni aceptación mixta. La API HTTP `/v1`, las versiones de configuración firmada y este número de cable son espacios independientes. El prólogo Noise cambia a `"atenea-one/v2" || network_id[16]`.

| Tipo | Offset y campo | Tamaño total |
| --- | --- | --- |
| INIT, `type=1` | 0 version; 1 type; 2 sender_index u32; 6 noise_msg1[108]; 114 mac1[16]; 130 mac2[16] | **146**, `INIT_LEN` |
| RESP, `type=2` | 0 version; 1 type; 2 sender_index u32; 6 receiver_index u32; 10 noise_msg2[52] | **62**, `RESP_LEN` |
| DATA, `type=3` | 0 version; 1 type; 2 receiver_index u32; 6 counter u64; 14 ciphertext y tag[16] | **30 + longitud del payload** |
| COOKIE_REPLY, `type=4` | 0 version; 1 type; 2 receiver_index u32; 6 nonce[24]; 30 encrypted_cookie[32] | **62**, `COOKIE_REPLY_LEN` |

INIT/RESP/COOKIE_REPLY exigen longitud exacta, sin padding ni bytes sobrantes. DATA admite payload vacío (keepalive), hasta MTU configurada; MTU por defecto 1400, rango 1280..1500, datagrama máximo absoluto 1530. Tipo desconocido, longitud inválida, MAC o AEAD inválidos: descarte silencioso. Un parser devuelve el error local apropiado, nunca un error al par. STUN conserva su discriminación separada; no se interpreta un paquete de versión incorrecta como INIT.

`noise_msg1 = e[32] || encrypted_s[48] || encrypted_payload[28]`; payload Noise = sender_index u32 y timestamp u64 Unix. `noise_msg2 = e[32] || encrypted_payload[20]`; payload = sender_index u32. Los índices externos deben coincidir con los autenticados por Noise, como antes. MAC1/MAC2 **no forman parte** del transcript Noise ni cambian sus tamaños.

## 2. Primitivas y bytes exactos

`H(x)=BLAKE2s-256(x)` sin clave. `M(k,x)=BLAKE2s` **en modo keyed con digest de 16 bytes**; no es `H(k||x)[0..16]`, ni truncar un digest de 32 bytes. Comparar MACs y cookies en tiempo constante. La clave de MAC1 es pública:

```
k_mac1 = H("label-mac1" || responder_static_public[32])
mac1   = M(k_mac1, INIT[0..114])
mac2   = M(cookie[16], INIT[0..130])
```

Sin cookie disponible y vigente para ese origen/ruta, `mac2 = 00` repetido 16 veces. MAC1 incluye versión, tipo, índice y mensaje Noise; MAC2 también incluye MAC1. Una cookie nueva solo cambia MAC2: se reenvía el mismo INIT, índice, timestamp y estado Noise pendientes, sin regenerar efímeras ni extender el intento.

Usar las implementaciones de BLAKE2s ya presentes (`blake2`) y **XChaCha20Poly1305 del paquete `chacha20poly1305 0.10.1` ya fijado en Cargo.lock y usado transitivamente por snow**, que exporta `XChaCha20Poly1305`/`XNonce` sin característica adicional. Hacer explícita esta dependencia en crypto reutiliza el paquete instalado, no incorpora otra biblioteca ni una primitiva propia. Noise sigue usando ChaChaPoly con su nonce ordinario; XChaCha se usa exclusivamente para la respuesta de cookie. No emular XChaCha ni reducir el nonce a 96 bits.

## 3. Origen, secreto y rotación

El respondedor mantiene un secreto aleatorio independiente `secret[32]` generado por el RNG del SO al arrancar, y lo sustituye cada **120 s de tiempo monotónico**. Conserva únicamente el anterior hasta la siguiente rotación. Si pasan dos períodos sin ejecutar el temporizador, se descarta el anterior: no se alarga su vida. Un reinicio invalida todas sus cookies. No se persisten ni registran secretos/cookies.

```
cookie = M(secret, source)
```

Codificación canónica de `source` (dominios distintos, sin longitud implícita variable):

- Directo IPv4: `0x01 || ipv4[4] || port u16` (7 bytes).
- Directo IPv6: `0x02 || ipv6[16] || port u16` (19 bytes).
- Relay: `0x03 || relay_id[16] || network_id[16] || sender_device_id[16] || receiver_device_id[16]` (65 bytes).

Normalizar IPv4-mapped IPv6 a IPv4 antes de codificar; IPv6 scoped/link-local no se admite como endpoint. Las cookies directas se ligan a IP **y puerto**, no a /64 (la agrupación /64 solo es para cuotas). En relay, los identificadores proceden de la concesión local y de la asignación emparejada que entrega `RelayEvent::Rx`, no de una supuesta IP embebida ni de campos no autenticados de INIT. El relay_id viaja por la interfaz cliente→engine junto con peer y transporte; el contexto de red/dispositivo receptor viene de la configuración local. UDP y TLS del mismo relay/par comparten `source`. El relay no confiable puede atribuir datos a otro par de su misma concesión, pero ello no autentica Noise ni cambia su autorización; no hay prueba E2E de retorno de una IP remota oculta por el relay.

Para verificar MAC2 se calculan ambas cookies candidatas (secreto vigente/anterior) y sus MAC2; basta una coincidencia. El cliente conserva cookies como máximo **120 s desde su recepción**, por respondedor estático y ruta: endpoint directo o `(relay_id, network_id, src, dst)`; nunca comparte una cookie directa entre candidatos. Se borran al cambiar clave, lease o configuración que retire el par. El respondedor decide la validez real: una cookie en caché puede haber caducado por rotación/reinicio y provocará otra respuesta, no aceptación forzada.

## 4. Respuesta de cookie y recepción

Solo se emite tras MAC1 correcto y bajo carga (§5), sin DH, autorización Noise ni asignación de sesión:

```
receiver_index = INIT.sender_index
k_reply = H("label-cookie" || responder_static_public[32] || INIT.mac1[16])
nonce = RNG_SO(24)
header = 0x02 || 0x04 || receiver_index u32 || nonce[24]
aad = header || INIT.mac1[16]
encrypted_cookie = XChaCha20Poly1305.Encrypt(k_reply, nonce, cookie[16], aad)
COOKIE_REPLY = header || encrypted_cookie
```

El AEAD produce ciphertext[16] seguido de tag[16]. Nonce independiente aleatorio por emisión, incluso al contestar el mismo INIT; no reutilizar una respuesta como generador de nonce. Para `q` emisiones bajo la misma clave, probabilidad de colisión ≤ `q(q−1)/2^193`; incluso `q=2^64` da menos de `2^-65`. El límite global de respuestas (§5), la caducidad de intentos y las efímeras frescas mantienen el uso muy por debajo. Fallo del RNG: no emitir (y hacer visible el fallo operativo), nunca nonce cero ni contador reiniciado. No necesita contadores persistentes.

El iniciador busca un intento **todavía pendiente** por receiver_index, ruta de recepción y respondedor esperado; usa el MAC1 guardado de ese INIT para derivar la clave y AAD. Un índice desconocido, camino diferente o AEAD fallido se ignora sin cancelar el intento ni borrar cookies previas. Una respuesta válida almacena la cookie y permite reintentar por **ese mismo camino** dentro del presupuesto existente (máximo un reintento por ronda y candidato, sin superar 12 envíos por candidato ni 10 s por intento). No crea sesión, no confirma endpoint, no marca sano el camino, no reinicia el sondeo/backoff y nunca dispara una respuesta a otra cookie. No se procesa si ya terminó el handshake. Un tercero que conozca la estática pública puede fabricar una respuesta cifrada para un MAC1 observado: esto es un mecanismo de disponibilidad, **no autenticación del respondedor**; Noise es quien autentica.

## 5. Política de carga y orden de validación

Configuración local del agente: `handshake_cookie_threshold_per_second` (entero positivo, valor por defecto **100**, nunca 0/desactivado). El respondedor cuenta INIT con longitud y MAC1 válidos en ventanas no solapadas de un segundo monotónico por clave estática local; al superar el umbral, el INIT actual y los restantes de esa ventana requieren MAC2. El contador satura sin desbordar. Un MAC2 válido no evita contar. Se mantiene únicamente contador y comienzo de ventana, sin tabla por emisor. No se usa tiempo Unix para activar/desactivar carga.

Orden obligatorio, incluso para retransmisiones y pares conocidos:

1. Versión v2; tipo; longitud exacta y límite absoluto.
2. Verificar MAC1 con la estática local; si falla, descartar antes de tocar cupos DH/estado por origen.
3. Contar para la carga. Bajo carga: verificar MAC2 con secreto vigente/anterior. Si falla o vale cero, emitir COOKIE_REPLY solo si lo permiten los cupos de respuesta y **terminar sin DH ni consumir cupo DH global de desconocidos**.
4. Aplicar los límites pre-DH existentes: por par configurado para contexto conocido/relay; por IP (IPv6 /64) y global para orígenes desconocidos, tablas acotadas. No sustituirlos por MAC1 o cookie.
5. DH y descifrado Noise; verificar clave estática autorizada, lease, red/prólogo, igualdad de índices y frescura (±30 s, marca mayor que la última salvo repetición idéntica con respuesta cacheada, y suelo de arranque). Solo entonces aceptar/contestar RESP.

Las respuestas de cookie tienen limitación **separada** de la de DH: por IP IPv4/IPv6 /64 o contexto relay/par, 20/s y ráfaga 20; global 100/s y ráfaga 100. Tabla por origen de máximo 4096 entradas; si no hay espacio ni entrada recuperable, no se responde. Reutilizar la política de recuperación de cubos repuestos existente. Sin colas ni asignación de sesión por origen no validado. La respuesta de 62 bytes es menor que INIT de 146. Sin carga MAC2 puede ser cero o inválido: no es requisito de autenticación y no se impone un RTT extra. La primera ventana admite hasta el umbral por la política normal; la cookie no garantiza disponibilidad frente a saturación de ancho de banda ni a un emisor capaz de recibir cookies.

## 6. Garantías y límites

- MAC1 permite descarte barato antes de DH si el emisor no conoce la estática pública del respondedor. **No es un secreto ni prueba de identidad**: cualquier conocedor puede generar MAC1.
- Bajo carga la cookie dificulta consumir DH con IP suplantada, especialmente para pares nuevos cuyo origen aún no se conoce; no autentica al peer y no permite saltarse cuotas o autorización.
- Se mantienen Noise IK, autorización por configuración, antirreplay de 2048, rekey, un único espacio de contadores por sesión/camino y confirmación inicial del endpoint solo con RESP autenticada/datos. MAC1/MAC2 no autentican la cabecera DATA.
- El roaming de sesiones establecidas sigue actualizando el endpoint con datos autenticados sin desafío de retorno adicional (riesgo residual aceptado, tipo WireGuard). Esta cookie solo protege INIT: no pretende corregir roaming, STUN ni SYN/TLS flood.

## 7. Vectores reproducibles y aceptación

Los resultados hex se insertarán **por la implementación antes de declarar conformidad**; este documento no inventa resultados ni afirma que hayan sido ejecutados. Generar con dos implementaciones independientes cuando sea posible, fijando todas las entradas (el RNG fijo es solo del vector):

1. `responder_private=42` repetido 32 bytes; pública = X25519(basepoint, private) con clamping normal. `network_id=07`×16, `sender_index=0x01020304`, `noise_msg1=00,01,...,6b` (108 bytes, vector de envoltorio, no handshake Noise válido). Construir prefijo `02 01 01 02 03 04 || noise_msg1`, calcular `k_mac1`, MAC1, INIT sin cookie (MAC2 cero).
2. `secret=00,01,...,1f`, origen IPv4 `192.0.2.1:51820` (`01 c0 00 02 01 ca 6c`); calcular cookie y MAC2 sobre el INIT anterior.
3. Origen IPv6 `[2001:db8::1]:51820` (familia 02, 16 bytes de la dirección, `ca6c`); versión IPv4-mapped debe producir el vector IPv4 anterior. Relay source = `03 || 11×16 || 07×16 || 22×16 || 33×16`; vector independiente y MAC2 distinto.
4. `nonce=00,01,...,17` (24 bytes), cookie IPv4 y MAC1 del vector 1: calcular k_reply, AAD, ciphertext/tag y COOKIE_REPLY. Cambiar un byte de índice, nonce, MAC1, tag o pública debe impedir descifrar con el contexto original.
5. Vector de handshake real adicional: iniciador private `41`×32, respondedor `42`×32, efímeras `43`×32 y `44`×32 inyectadas solo en tests, red `07`×16, índices 0x01020304/0x05060708, timestamp 1700000000, prólogo v2; publicar msg1/msg2 e INIT/RESP completos, y confirmar transporte en ambas direcciones.

Generados offline con una implementación independiente en Python: `hashlib.blake2s`
keyed/digest_size=16, X25519/ChaCha20Poly1305 de `cryptography`, HChaCha20 y
transcript/KDF Noise calculados por separado de snow. Los tests Rust comparan
estas salidas con blake2/XChaCha20Poly1305/snow; su ejecución se informa aparte.
Transporte del vector 5: contador0, payloads ASCII `iniciador` y `respondedor`,
ciphertext seguido de tag (sin cabecera DATA).

| Salida | Hex |
| --- | --- |
| `public` | `132c442be010fbd57e72603328aa76e71fccc1503aae219327d14d9c9993f472` |
| `k_mac1` | `4a4e96346463c2b0d438350c05db786c1c29ddd6016ade968070a94d76d07b81` |
| `prefix` | `020101020304000102030405060708090a0b0c0d0e0f101112131415161718191a1b1c1d1e1f202122232425262728292a2b2c2d2e2f303132333435363738393a3b3c3d3e3f404142434445464748494a4b4c4d4e4f505152535455565758595a5b5c5d5e5f606162636465666768696a6b` |
| `mac1` | `cebc78788406c7247bf69fd44ac2aa49` |
| `init_zero` | `020101020304000102030405060708090a0b0c0d0e0f101112131415161718191a1b1c1d1e1f202122232425262728292a2b2c2d2e2f303132333435363738393a3b3c3d3e3f404142434445464748494a4b4c4d4e4f505152535455565758595a5b5c5d5e5f606162636465666768696a6bcebc78788406c7247bf69fd44ac2aa4900000000000000000000000000000000` |
| `ipv4_source` | `01c0000201ca6c` |
| `ipv4_cookie` | `da42e779cef85f0a2480be26f100de3b` |
| `ipv4_mac2` | `72713cbab51e7319cfff424fd7d9c684` |
| `ipv4_init` | `020101020304000102030405060708090a0b0c0d0e0f101112131415161718191a1b1c1d1e1f202122232425262728292a2b2c2d2e2f303132333435363738393a3b3c3d3e3f404142434445464748494a4b4c4d4e4f505152535455565758595a5b5c5d5e5f606162636465666768696a6bcebc78788406c7247bf69fd44ac2aa4972713cbab51e7319cfff424fd7d9c684` |
| `ipv6_source` | `0220010db8000000000000000000000001ca6c` |
| `ipv6_cookie` | `2fce57bfea5bfbc7872aa15bd60a34b7` |
| `ipv6_mac2` | `61cdae49ee7afab1572eacbb464cdc1a` |
| `ipv6_init` | `020101020304000102030405060708090a0b0c0d0e0f101112131415161718191a1b1c1d1e1f202122232425262728292a2b2c2d2e2f303132333435363738393a3b3c3d3e3f404142434445464748494a4b4c4d4e4f505152535455565758595a5b5c5d5e5f606162636465666768696a6bcebc78788406c7247bf69fd44ac2aa4961cdae49ee7afab1572eacbb464cdc1a` |
| `relay_source` | `0311111111111111111111111111111111070707070707070707070707070707072222222222222222222222222222222233333333333333333333333333333333` |
| `relay_cookie` | `e0a93cb16d55793b08b08ca4101d422a` |
| `relay_mac2` | `26037d42fda06af340c8dbf22d716a01` |
| `relay_init` | `020101020304000102030405060708090a0b0c0d0e0f101112131415161718191a1b1c1d1e1f202122232425262728292a2b2c2d2e2f303132333435363738393a3b3c3d3e3f404142434445464748494a4b4c4d4e4f505152535455565758595a5b5c5d5e5f606162636465666768696a6bcebc78788406c7247bf69fd44ac2aa4926037d42fda06af340c8dbf22d716a01` |
| `k_reply` | `0c2710e5332697ef7642e7bea25fba97fbf77558d4817e1aeaa21ea668dbc48b` |
| `nonce` | `000102030405060708090a0b0c0d0e0f1011121314151617` |
| `aad` | `020401020304000102030405060708090a0b0c0d0e0f1011121314151617cebc78788406c7247bf69fd44ac2aa49` |
| `encrypted_cookie` | `c67047b8d08597722a070c85154a21b75b5012965a729445ec2fc192b8514ccb` |
| `cookie_reply` | `020401020304000102030405060708090a0b0c0d0e0f1011121314151617c67047b8d08597722a070c85154a21b75b5012965a729445ec2fc192b8514ccb` |
| `noise_msg1` | `cdefd8783a91b446640e2e1f95599db35e484a0071bd2182b3b60d0812c10c70aba37041ab54ea71c2832ee21c25f082288288e99b5c54f929110578a596816c7130ae5dc9328115853afebfe2757971c891f9025870d74efda892fe4c10545be54583b6340a4d034702dc72` |
| `noise_msg2` | `ff2ee45601ec1b67310c7790404585ae697331eee1c1f8cf2419731c1fff3e6ba6124129bf0d579bcd170037a5a3ef1f697e1c74` |
| `real_init` | `020101020304cdefd8783a91b446640e2e1f95599db35e484a0071bd2182b3b60d0812c10c70aba37041ab54ea71c2832ee21c25f082288288e99b5c54f929110578a596816c7130ae5dc9328115853afebfe2757971c891f9025870d74efda892fe4c10545be54583b6340a4d034702dc7239ee9042ad4d2b25b4e40577290bb4b600000000000000000000000000000000` |
| `real_resp` | `02020506070801020304ff2ee45601ec1b67310c7790404585ae697331eee1c1f8cf2419731c1fff3e6ba6124129bf0d579bcd170037a5a3ef1f697e1c74` |
| `transport_i` | `9090c346f6b0492f94917bd22155fb565bb4603a423cfd10ac` |
| `transport_r` | `de17e74fc616751726ea23db3fdc51cac6c5dcfacb7ff81839c5b9` |

Pruebas obligatorias: tamaños exactos y round-trip; v1/versión desconocida descartada; rechazo de mutaciones/truncamiento/colas; MAC1 inválido no llama DH ni consume cupos DH; umbral y frontera de ventana; MAC2 actual/anterior, caducidad y salto de dos rotaciones; cookies de otro puerto/IP/relay/par/red rechazadas; spoof flood bajo carga no consume el cubo DH de desconocidos; límites y memoria de cookie replies; sin DH hasta retorno de cookie; respuesta maliciosa no cancela intento ni confirma ruta; caché por candidato y límite de reintentos; NAT/rebind obtiene nueva cookie; no se alteran antirreplay ni las reglas de endpoint/roaming.

## 8. Riesgos y decisiones abiertas

No quedan decisiones funcionales abiertas para implementar este contrato. Umbral 100/s y cuotas de respuesta son parámetros operativos que deben medirse, no garantías contra un DDoS. La revisión criptográfica externa debe revisar dominios, construcción keyed/digest-16, AAD y límites antes de aprobar producción. MAC1/clave de respuesta públicas permiten interferencia de disponibilidad por quien conoce la pública; un relay puede manipular metadatos de origen; ninguna cookie convierte a esos actores en confiables. La tabla de vectores pendiente es una entrega exigida a la implementación, no un permiso para sustituir pruebas con datos inventados.
