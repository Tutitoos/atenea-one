ALTER TABLE devices
    ADD COLUMN name_source text NOT NULL DEFAULT 'admin' CHECK (name_source IN ('device','admin')),
    ADD COLUMN hostname text;
