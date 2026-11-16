ALTER TABLE devices ADD COLUMN versions jsonb NOT NULL DEFAULT '[]', ADD COLUMN versions_at timestamptz;
