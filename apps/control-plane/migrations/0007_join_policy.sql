ALTER TABLE networks
    ADD COLUMN join_policy text NOT NULL DEFAULT 'cerrada' CHECK (join_policy IN ('cerrada','con_solicitud'));
ALTER TABLE devices
    ADD COLUMN join_origin text NOT NULL DEFAULT 'token' CHECK (join_origin IN ('token','request'));
CREATE INDEX devices_pending_requests_idx ON devices (network_id, enrolled_at)
    WHERE status = 'pending' AND join_origin = 'request';
