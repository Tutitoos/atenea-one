# Código de red v1

- **Estado:** implementado en `crates/identity` (`join_code`), ADR-013 fase 1. La **extensión de conectividad** (STUN y hash de la CA del relay, banderas 1 y 2) es el formato de [ADR-018](../architecture/ADR-018-union-solo-con-el-codigo.md) fase 1: **solo el formato**; el control plane todavía no la emite y el CLI todavía no la usa. Todos los valores de los ejemplos son sintéticos.
- **Implementa:** `JoinCode::encode` / `JoinCode::decode` y `anchor_fingerprint`.

Un **código de red** es un texto copiable que contiene lo necesario para pedir la unión a una red: dirección del servidor, identificador de red y **raíz de confianza** (`root_anchor`), más una suma de comprobación. Opcionalmente lleva un token de enrolamiento de un uso.

## 1. Formato binario

Enteros de longitud de 1 byte; las longitudes son de bytes UTF-8.

```
version(1) | flags(1) | len_url(1) url | network_id(16) | root_anchor(32)
           | len_name(1) name | [len_stun(1) stun_host [| relay_ca_sha256(32)]] | [len_token(1) token] | checksum(4)
```

- `version` = `1`. Cualquier otra se rechaza.
- `flags`: bit 0 = lleva token; bit 1 = lleva el **bloque de conectividad**; bit 2 = el bloque lleva además el **hash de la CA del relay**. El resto (bits 3 a 7) DEBEN ser 0 y se rechazan si no (`ReservedFlags`). El bit 2 sin el bit 1 es inválido.
- El bloque de conectividad va **tras el nombre y antes del token**. `stun_host` es un nombre o una IPv4 **sin puerto** (el agente usa los puertos 3478 y 3479 de ese servidor). `relay_ca_sha256` es el SHA-256 del certificado de la CA del relay en DER; falta si el relay usa una CA pública.
- `token` está presente si y solo si el bit 0 está activo.
- `checksum` = primeros 4 bytes de `BLAKE2s-256(b"atenea-one/join-code/v1" || payload_sin_checksum)`.

**Compatibilidad.** Un código sin los bits 1 y 2 es **byte a byte idéntico** al de la versión anterior (los vectores de §5 no cambian). Un decodificador anterior, que solo conoce el bit 0, rechaza un código con conectividad con `ReservedFlags` («el código usa indicadores reservados») en lugar de interpretarlo mal.

## 2. Formato de texto

`ATENEA1-` + base32 RFC 4648 (alfabeto `A–Z2–7`, **sin relleno**) del payload completo, agrupado en bloques de 6 caracteres separados por `-`. El decodificador ignora mayúsculas/minúsculas, espacios, saltos de línea y guiones, y exige el prefijo `ATENEA1` tras normalizar. Los bits de relleno del último carácter deben ser cero (una sola codificación por valor). Longitud máxima del texto: **512 caracteres**; se comprueba antes de decodificar **y al codificar** (con el bloque de conectividad el peor caso no cabe: `encode` devuelve `TooLong` en lugar de emitir un código que el decodificador rechazaría).

## 3. Validación (al codificar y al decodificar)

| Campo | Regla |
| --- | --- |
| `url` | 1..=100 bytes; ASCII visible; empieza por `https://`; host no vacío; sin userinfo (`@`), query (`?`), fragmento (`#`), espacios ni controles; sin `/` final; admite puerto y prefijo de ruta (`https://control.example.test/prefijo`) |
| `name` | 1..=48 bytes UTF-8; igual tras `trim` (no se recorta en silencio); sin controles (Cc) ni caracteres bidireccionales U+202A–U+202E y U+2066–U+2069 (mismo criterio que los nombres de dispositivo) |
| `token` | solo con el bit 0; 1..=64 bytes ASCII visibles (base64url) |
| `stun_host` | solo con el bit 1; 1..=100 bytes de `A–Z a–z 0–9 . -` (nombre o IPv4, **sin puerto**; no se admite IPv6). Es la misma regla que `--stun-host` del CLI. Un fallo es `InvalidConnectivity` |
| `relay_ca_sha256` | solo con los bits 1 y 2; exactamente 32 bytes. Incompleto es `Truncated` |
| resto | sin bytes sobrantes; suma de comprobación correcta |

Cualquier fallo devuelve un error sin pánico. Los errores y el `Debug` de `JoinCode` **nunca incluyen el token**.

## 4. Qué garantiza y qué no

- **Detecta erratas**: cualquier alteración de un solo carácter se rechaza (probado exhaustivamente); una alteración arbitraria solo pasa por colisión del checksum de 32 bits.
- **NO autentica.** El checksum no es una firma: cualquiera puede fabricar un código válido. La confianza viene de que el código llegó por un **canal que el usuario controla** (el administrador se lo entregó en persona, por un mensaje de confianza…).
- **Por qué lleva la raíz.** El cliente fija la `root_anchor` del código como ancla de confianza desde el primer contacto: no confía a ciegas en lo que el servidor diga después (sin TOFU). Junto al código se muestra su **huella** para compararla a mano: 16 bytes de `BLAKE2s-256(b"atenea-one/anchor-fingerprint/v1" || root_anchor)` en grupos de 4 hex separados por `-` (mismo formato que la huella de dispositivo, dominio propio).
- **Un código sin token** no concede acceso: solo permite enviar una solicitud de unión a una red con política `con_solicitud`, que un administrador aprueba comparando la huella del dispositivo. **Un código con token** equivale a una credencial de un uso: debe tratarse como secreto.
- Límites: no cifra, no caduca por sí mismo (el token sí, en el servidor) y no se puede revocar; un código filtrado se invalida cambiando la política de la red o, con token, esperando su caducidad.
- **La conectividad tampoco autentica.** El STUN y el hash de la CA del relay viajan en el mismo texto, sin firma: quien fabrique el código puede poner los suyos. Por eso el hash **no** sustituye a la comparación de la huella de la raíz por otro canal; su función es que la CA que el cliente descargue después **coincida con la que el código anuncia**, de modo que el servidor no pueda cambiarla por otra (ADR-018 § 3.2).

## 5. Vector de prueba fijo

Entradas sintéticas: `url = https://control.example.test`, `network_id = 01 02 … 10` (bytes 1 a 16), `root_anchor = 20 21 … 3f` (bytes 0x20 a 0x3f), `name = Red de ejemplo`.

Sin token:

```
ATENEA1-AEABY2-DUORYH-GORPF5-RW63TU-OJXWYL-TFPBQW-24DMMU-XHIZLT-OQAQEA-YEAUDA-OCAJBI-FQYDIO-B4ICAI-JCEMSC-KJRHFA-USUKZM-FUXC6M-BRGIZT-INJWG4-4DSOR3-HQ6T4P-YOKJSW-IIDEMU-QGK2TF-NVYGY3-5PWBEV-4
```

Con `token = dG9rZW4tZGUtZWplbXBsbw`:

```
ATENEA1-AEARY2-DUORYH-GORPF5-RW63TU-OJXWYL-TFPBQW-24DMMU-XHIZLT-OQAQEA-YEAUDA-OCAJBI-FQYDIO-B4ICAI-JCEMSC-KJRHFA-USUKZM-FUXC6M-BRGIZT-INJWG4-4DSOR3-HQ6T4P-YOKJSW-IIDEMU-QGK2TF-NVYGY3-YWMRDT-S4S2K4-2HIWSH-KV2FUV-3QNRRF-QQTTMJ-3TPSJM-GQ
```

Huella de la raíz del vector: `6c4b-fd0b-2585-9aae-61ad-9159-3650-be21`.

La prueba `join_code::tests::vector_fijo` y `anchor_tests::huella_de_raiz_vector_fijo` verifican estos valores byte a byte.

### Con conectividad

Mismas entradas, más `stun_host = relay.example.test` y, cuando se indica, `relay_ca_sha256 = 60 61 … 7f` (bytes 0x60 a 0x7f).

Solo STUN (bandera 1):

```
ATENEA1-AEBBY2-DUORYH-GORPF5-RW63TU-OJXWYL-TFPBQW-24DMMU-XHIZLT-OQAQEA-YEAUDA-OCAJBI-FQYDIO-B4ICAI-JCEMSC-KJRHFA-USUKZM-FUXC6M-BRGIZT-INJWG4-4DSOR3-HQ6T4P-YOKJSW-IIDEMU-QGK2TF-NVYGY3-YSOJSW-YYLZFZ-SXQYLN-OBWGKL-TUMVZX-I2T7VG-KA
```

STUN y hash de la CA (banderas 1 y 2):

```
ATENEA1-AEDBY2-DUORYH-GORPF5-RW63TU-OJXWYL-TFPBQW-24DMMU-XHIZLT-OQAQEA-YEAUDA-OCAJBI-FQYDIO-B4ICAI-JCEMSC-KJRHFA-USUKZM-FUXC6M-BRGIZT-INJWG4-4DSOR3-HQ6T4P-YOKJSW-IIDEMU-QGK2TF-NVYGY3-YSOJSW-YYLZFZ-SXQYLN-OBWGKL-TUMVZX-IYDBMJ-RWIZLG-M5UGS2-TLNRWW-433QOF-ZHG5DV-OZ3XQ6-L2PN6H-27T7CE-H4DZI
```

Con `token = dG9rZW4tZGUtZWplbXBsbw`, STUN y hash de la CA (banderas 0, 1 y 2):

```
ATENEA1-AEDRY2-DUORYH-GORPF5-RW63TU-OJXWYL-TFPBQW-24DMMU-XHIZLT-OQAQEA-YEAUDA-OCAJBI-FQYDIO-B4ICAI-JCEMSC-KJRHFA-USUKZM-FUXC6M-BRGIZT-INJWG4-4DSOR3-HQ6T4P-YOKJSW-IIDEMU-QGK2TF-NVYGY3-YSOJSW-YYLZFZ-SXQYLN-OBWGKL-TUMVZX-IYDBMJ-RWIZLG-M5UGS2-TLNRWW-433QOF-ZHG5DV-OZ3XQ6-L2PN6H-27T7CZ-SEOOLS-LJLTI5-C2I5KX-IWSXOB-WGEWCC-ONRHPE-HPRAEA
```

La prueba `join_code::tests::vectores_de_conectividad` los verifica byte a byte, y `toda_alteracion_de_un_caracter_se_rechaza` los incluye.
