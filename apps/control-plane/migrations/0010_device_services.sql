ALTER TABLE devices ADD COLUMN services jsonb NOT NULL DEFAULT '[]', ADD COLUMN services_at timestamptz;
