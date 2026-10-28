ALTER TABLE devices ADD COLUMN role text NOT NULL DEFAULT 'device' CHECK (role IN ('device', 'server'));
ALTER TABLE enrollment_tokens ADD COLUMN role text NOT NULL DEFAULT 'device' CHECK (role IN ('device', 'server'));
