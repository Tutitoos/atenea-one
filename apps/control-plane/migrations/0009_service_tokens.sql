-- ADR-020: tokens de servicio de solo lectura. Cada token es un usuario interno (kind='service') con membresía member.
ALTER TABLE users ADD COLUMN kind text NOT NULL DEFAULT 'oidc' CHECK (kind IN ('oidc','service'));
ALTER TABLE users ALTER COLUMN oidc_issuer DROP NOT NULL, ALTER COLUMN oidc_subject DROP NOT NULL;
ALTER TABLE users ADD CONSTRAINT users_oidc_identity CHECK (kind = 'service' OR (oidc_issuer IS NOT NULL AND oidc_subject IS NOT NULL));
CREATE TABLE service_tokens (
  id uuid PRIMARY KEY,
  user_id uuid NOT NULL REFERENCES users ON DELETE CASCADE,
  org_id uuid NOT NULL REFERENCES organizations ON DELETE CASCADE,
  name text NOT NULL,
  token_hash bytea NOT NULL UNIQUE CHECK (octet_length(token_hash) = 32),
  created_by uuid NOT NULL REFERENCES users,
  created_at timestamptz NOT NULL DEFAULT now(),
  expires_at timestamptz NOT NULL,
  last_used_at timestamptz,
  revoked_at timestamptz
);
CREATE INDEX service_tokens_org ON service_tokens (org_id, created_at);
