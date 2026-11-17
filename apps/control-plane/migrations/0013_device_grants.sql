-- ADR-026: administración delegada por dispositivo. Una concesión por dispositivo (y por tanto por red): permisos y alcance.
CREATE TABLE device_grants (
  device_id uuid PRIMARY KEY REFERENCES devices ON DELETE CASCADE,
  network_id uuid NOT NULL REFERENCES networks ON DELETE CASCADE,
  scopes jsonb NOT NULL,
  scope_kind text NOT NULL CHECK (scope_kind IN ('enrolled','devices','network')),
  scope_devices jsonb NOT NULL DEFAULT '[]',
  granted_by uuid NOT NULL REFERENCES users,
  granted_at timestamptz NOT NULL DEFAULT now()
);
CREATE INDEX device_grants_network ON device_grants (network_id);
-- Autoría delegada: `created_by` (usuario) pasa a admitir nulo y `created_by_device` lo sustituye cuando el autor es un
-- dispositivo. El binario anterior solo escribe `created_by` y no lee ninguna de las dos columnas.
ALTER TABLE enrollment_tokens ADD COLUMN created_by_device uuid REFERENCES devices ON DELETE SET NULL;
ALTER TABLE enrollment_tokens ALTER COLUMN created_by DROP NOT NULL;
ALTER TABLE devices ADD COLUMN enrolled_by_device uuid REFERENCES devices ON DELETE SET NULL;
ALTER TABLE access_policies ADD COLUMN created_by_device uuid REFERENCES devices ON DELETE SET NULL;
ALTER TABLE access_policies ALTER COLUMN created_by DROP NOT NULL;
