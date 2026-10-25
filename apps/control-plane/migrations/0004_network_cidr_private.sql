-- F7: los CIDR de red solo pueden ser privados (ADR-007) y no más amplios que /8 (IPv4) ni /32 (IPv6).
-- NOT VALID: se aplica a altas y modificaciones, sin reescribir ni invalidar redes existentes.
-- Para validar las existentes (ADR-009 §7): ALTER TABLE networks VALIDATE CONSTRAINT networks_ipv4_private;
-- (y networks_ipv6_private). Falla, sin cambiar datos, si hay una red fuera de rango.
ALTER TABLE networks ADD CONSTRAINT networks_ipv4_private CHECK (
    masklen(ipv4_cidr) >= 8 AND (
        ipv4_cidr <<= '10.0.0.0/8'::cidr OR ipv4_cidr <<= '172.16.0.0/12'::cidr
        OR ipv4_cidr <<= '192.168.0.0/16'::cidr OR ipv4_cidr <<= '100.64.0.0/10'::cidr)
) NOT VALID;
ALTER TABLE networks ADD CONSTRAINT networks_ipv6_private CHECK (
    masklen(ipv6_cidr) >= 32 AND ipv6_cidr <<= 'fc00::/7'::cidr
) NOT VALID;
