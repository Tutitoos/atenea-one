-- Copyright 2026 Tutitoos
-- SPDX-License-Identifier: Apache-2.0
-- Direccionamiento elegible al crear una red: IPv6 opcional y rango de reparto IPv4.
-- Aditiva: las redes existentes conservan su IPv6 y siguen repartiendo todo su bloque IPv4.
ALTER TABLE networks ALTER COLUMN ipv6_cidr DROP NOT NULL;
ALTER TABLE networks
  ADD COLUMN ipv4_pool_start inet,
  ADD COLUMN ipv4_pool_end inet,
  ADD CONSTRAINT networks_ipv4_pool CHECK (
    (ipv4_pool_start IS NULL AND ipv4_pool_end IS NULL)
    OR (family(ipv4_pool_start) = 4 AND family(ipv4_pool_end) = 4
        AND masklen(ipv4_pool_start) = 32 AND masklen(ipv4_pool_end) = 32
        AND ipv4_pool_start <= ipv4_pool_end
        AND ipv4_pool_start << ipv4_cidr AND ipv4_pool_end << ipv4_cidr)
  );
