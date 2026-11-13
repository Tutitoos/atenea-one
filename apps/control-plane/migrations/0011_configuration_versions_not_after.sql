-- Retención y recuento de leases acotados por fecha (panel::purge_versions y panel::sampler).
-- CREATE INDEX normal bloquea las escrituras de configuration_versions mientras se crea (SQLx lo ejecuta en
-- transacción): migrar antes de sustituir el binario y reservar una ventana si la tabla es grande.
CREATE INDEX IF NOT EXISTS configuration_versions_not_after
    ON configuration_versions(not_after);
