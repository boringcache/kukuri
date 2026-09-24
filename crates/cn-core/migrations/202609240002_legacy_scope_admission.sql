-- Rotate legacy CN work without opening every supported replica each pass.
CREATE INDEX idx_cn_index_supported_recent_demand
    ON cn_index.supported_topics (last_index_demand_at DESC, kind, id)
    WHERE last_index_demand_at IS NOT NULL;

CREATE TABLE cn_index.legacy_scope_cursor (
    id BOOLEAN PRIMARY KEY DEFAULT TRUE CHECK (id),
    last_kind TEXT NOT NULL DEFAULT '',
    last_scope_id TEXT NOT NULL DEFAULT ''
);
INSERT INTO cn_index.legacy_scope_cursor (id) VALUES (TRUE);
