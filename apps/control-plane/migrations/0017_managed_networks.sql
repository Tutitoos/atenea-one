-- ADR-029: redes gestionadas por el servidor. `root_wrapped` es la semilla de la raíz cifrada con la clave de envoltura
-- (nonce || texto cifrado, con el id de la red como dato asociado); nunca en claro. Aditiva: el binario anterior no lee
-- estas columnas y trata la red como una red con raíz propia (por eso no puede renovar su certificado).
ALTER TABLE networks
  ADD COLUMN root_mode text NOT NULL DEFAULT 'own' CHECK (root_mode IN ('own', 'managed')),
  ADD COLUMN root_wrapped bytea;
ALTER TABLE networks
  ADD CONSTRAINT networks_managed_root CHECK ((root_mode = 'managed') = (root_wrapped IS NOT NULL));
