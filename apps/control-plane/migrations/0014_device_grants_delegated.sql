-- ADR-027: un dispositivo con `write:grants` concede permisos a otros dispositivos de su alcance. `granted_by_device` es el
-- dispositivo que la dio (nulo si la dio un administrador) y su borrado arrastra las concesiones que dio. `granted_by`
-- sigue siendo no nulo: en una concesión delegada guarda al administrador que dio `write:grants` a quien la concede, así
-- que el binario anterior (que lee `granted_by` como uuid) sigue listándolas.
ALTER TABLE device_grants ADD COLUMN granted_by_device uuid REFERENCES devices ON DELETE CASCADE;
CREATE INDEX device_grants_by_device ON device_grants (granted_by_device) WHERE granted_by_device IS NOT NULL;
