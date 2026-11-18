-- ADR-028 D6/D7: pertenencia por red (`viewer` ve la red; `enroller` además genera códigos y aprueba) e invitaciones
-- por enlace de un solo uso. Es aditiva: el binario anterior no lee estas tablas y un usuario solo miembro de una red
-- no tiene membresía de organización, así que con él no ve nada.
CREATE TABLE network_members (
  network_id uuid NOT NULL REFERENCES networks ON DELETE CASCADE,
  user_id uuid NOT NULL REFERENCES users ON DELETE CASCADE,
  role text NOT NULL CHECK (role IN ('viewer', 'enroller')),
  invited_by uuid REFERENCES users ON DELETE SET NULL,
  created_at timestamptz NOT NULL DEFAULT now(),
  PRIMARY KEY (network_id, user_id)
);
CREATE INDEX network_members_user ON network_members (user_id);

-- Solo el SHA-256 del token; el valor en claro se muestra una vez al crearla.
CREATE TABLE network_invitations (
  id uuid PRIMARY KEY,
  network_id uuid NOT NULL REFERENCES networks ON DELETE CASCADE,
  token_hash bytea NOT NULL UNIQUE CHECK (octet_length(token_hash) = 32),
  role text NOT NULL CHECK (role IN ('viewer', 'enroller')),
  created_by uuid REFERENCES users ON DELETE SET NULL,
  created_at timestamptz NOT NULL DEFAULT now(),
  expires_at timestamptz NOT NULL,
  used_at timestamptz,
  used_by uuid REFERENCES users ON DELETE SET NULL,
  revoked_at timestamptz
);
CREATE INDEX network_invitations_network ON network_invitations (network_id, created_at DESC);
