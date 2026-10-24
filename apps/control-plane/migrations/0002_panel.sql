CREATE TABLE device_reports (
    device_id uuid PRIMARY KEY REFERENCES devices(id) ON DELETE CASCADE,
    reported_at timestamptz NOT NULL,
    nat_mapping text CHECK (nat_mapping IN ('independent','dependent','unknown')),
    paths jsonb NOT NULL
);
CREATE TABLE network_samples (
    network_id uuid NOT NULL REFERENCES networks(id) ON DELETE CASCADE,
    at timestamptz NOT NULL,
    total bigint NOT NULL,
    direct bigint NOT NULL,
    relay bigint NOT NULL,
    probing bigint NOT NULL,
    unknown bigint NOT NULL,
    offline bigint NOT NULL,
    PRIMARY KEY(network_id, at)
);
-- Los permisos se conceden mediante db::migrate al rol configurado; auditoría sigue inmutable.
