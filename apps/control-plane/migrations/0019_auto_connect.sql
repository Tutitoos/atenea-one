-- Copyright 2026 Tutitoos
-- SPDX-License-Identifier: Apache-2.0
-- ADR-031: conexiones automáticas por aprobación. Las redes existentes conservan su acceso manual: el ajuste
-- arranca desactivado y solo las redes nuevas lo tienen activo por defecto.
ALTER TABLE networks ADD COLUMN auto_connect boolean NOT NULL DEFAULT false;
ALTER TABLE networks ALTER COLUMN auto_connect SET DEFAULT true;
-- Orden de aprobación y ajuste vigente en esa aprobación. Los aprobados anteriores quedan en 0 y sin ajuste: no se
-- originan conexiones automáticas entre ellos.
ALTER TABLE devices ADD COLUMN approval_order bigint NOT NULL DEFAULT 0;
ALTER TABLE devices ADD COLUMN auto_connect_on_approval boolean NOT NULL DEFAULT false;
-- Capacidad explícita de configuración v3 (ADR-031): los agentes anteriores no la anuncian.
ALTER TABLE devices ADD COLUMN config_v3 boolean NOT NULL DEFAULT false;
-- Excepciones por pareja, sin dirección. device_a < device_b evita guardar la misma pareja dos veces.
CREATE TABLE network_auto_exceptions (
  network_id uuid NOT NULL REFERENCES networks ON DELETE CASCADE,
  device_a uuid NOT NULL REFERENCES devices ON DELETE CASCADE,
  device_b uuid NOT NULL REFERENCES devices ON DELETE CASCADE,
  PRIMARY KEY (network_id, device_a, device_b),
  CHECK (device_a < device_b)
);
CREATE INDEX network_auto_exceptions_device_b ON network_auto_exceptions (network_id, device_b);
