CREATE TABLE device_routes (
    id uuid PRIMARY KEY,
    network_id uuid NOT NULL REFERENCES networks(id) ON DELETE CASCADE,
    device_id uuid NOT NULL REFERENCES devices(id) ON DELETE CASCADE,
    prefix cidr NOT NULL,
    status text NOT NULL DEFAULT 'pending' CHECK (status IN ('pending','approved','rejected')),
    snat boolean,
    declared_at timestamptz NOT NULL DEFAULT now(),
    decided_at timestamptz,
    decided_by uuid REFERENCES users(id),
    UNIQUE(device_id,prefix),
    CHECK (status <> 'approved' OR snat IS NOT NULL)
);
CREATE INDEX device_routes_network ON device_routes(network_id,status);
ALTER TABLE networks ADD COLUMN dns_servers jsonb NOT NULL DEFAULT '[]';
ALTER TABLE networks ADD COLUMN dns_domains jsonb NOT NULL DEFAULT '[]';
ALTER TABLE devices ADD COLUMN routes_declared_at timestamptz;
ALTER TABLE device_reports ADD COLUMN forwarding_ok boolean;
ALTER TABLE device_reports ADD COLUMN dns text CHECK (dns IN ('applied','unsupported','none'));
ALTER TABLE device_reports ADD COLUMN routes jsonb;
