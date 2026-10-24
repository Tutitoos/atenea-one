# MTU del túnel

- **Valor por defecto:** 1400 bytes (`atenea_protocol::DEFAULT_MTU`). Rango admitido por el agente: 1280 (mínimo IPv6) a 1500.
- **Sobrecarga por paquete de datos:** 30 bytes (cabecera 14 + etiqueta AEAD 16, ADR-003) más la cabecera UDP y la IP de la red subyacente.

| Red subyacente | Datagrama en el cable con MTU 1400 | Cabe en 1500 |
| --- | --- | --- |
| IPv4 | 1400 + 30 + 8 + 20 = 1458 | sí (margen 42) |
| IPv6 | 1400 + 30 + 8 + 40 = 1478 | sí (margen 22) |

Con un MTU de túnel de 1500 hace falta un enlace subyacente de al menos 1558 (IPv4) o 1578 (IPv6); si no, los datagramas se fragmentan o se pierden. El fallback TLS/TCP (fase 5) añadirá su propia sobrecarga: se recalculará entonces.

## Comportamiento

- El kernel aplica el MTU de la interfaz: un paquete mayor no entra en el túnel. Con DF activo, el emisor recibe el error "mensaje demasiado largo" (IPv4) o el ICMPv6 "paquete demasiado grande" generados por el propio host; el agente no genera ICMP.
- El agente descarta datagramas de más de `MAX_MTU + 30` bytes y paquetes internos que no cuadran con su longitud IP declarada.
- Sin compresión ni relleno.

## Verificado

`tests/network-lab/lab.sh` comprueba que con MTU 1400 pasan paquetes de 1400 bytes con DF en IPv4 e IPv6 y se rechazan los de 1401. No se ha medido el MTU de caminos reales de Internet: se hará en la fase 4.
