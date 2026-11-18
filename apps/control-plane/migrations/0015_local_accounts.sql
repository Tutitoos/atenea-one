-- ADR-028: cuentas con correo y contraseña propias del control plane (identidad `local`), organización personal
-- automática y códigos de recuperación. Todo es aditivo: el binario anterior ignora las tablas y columnas nuevas.
ALTER TABLE users ADD COLUMN personal_org_done boolean NOT NULL DEFAULT false;
ALTER TABLE organizations ADD COLUMN personal boolean NOT NULL DEFAULT false;

-- `login` es el correo sin verificar, en minúsculas. No se copia a `users.email` (ADR-028 D3): solo un correo
-- verificado por un proveedor sirve de clave para añadir miembros.
CREATE TABLE local_credentials (
  user_id uuid PRIMARY KEY REFERENCES users ON DELETE CASCADE,
  login text NOT NULL UNIQUE CHECK (login = lower(login) AND length(login) BETWEEN 3 AND 254),
  password_hash text NOT NULL,
  failed_attempts integer NOT NULL DEFAULT 0,
  locked_until timestamptz,
  created_at timestamptz NOT NULL DEFAULT now()
);

-- Solo el SHA-256 del código; `used_at` lo hace de un solo uso.
CREATE TABLE recovery_codes (
  user_id uuid NOT NULL REFERENCES users ON DELETE CASCADE,
  code_hash bytea NOT NULL CHECK (octet_length(code_hash) = 32),
  used_at timestamptz,
  PRIMARY KEY (user_id, code_hash)
);
