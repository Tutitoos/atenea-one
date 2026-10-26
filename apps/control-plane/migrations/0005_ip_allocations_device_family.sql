-- Expand/contract (ADR-009 §7): migrar antes de sustituir el binario; conservar los índices únicos.
-- CREATE INDEX normal bloquea brevemente escrituras de ip_allocations; SQLx lo ejecuta en transacción.
CREATE INDEX IF NOT EXISTS ip_allocations_device_family
    ON ip_allocations(device_id, family) INCLUDE (addr);
