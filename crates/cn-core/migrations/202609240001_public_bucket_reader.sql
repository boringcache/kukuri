-- A bounded reader rotates through operator-supported public topics. Demand is advisory and
-- expires by query time; it never grants indexing authority or changes the supported set.
ALTER TABLE cn_index.supported_topics ADD COLUMN last_index_demand_at TIMESTAMPTZ;
CREATE INDEX idx_cn_index_supported_public_demand
    ON cn_index.supported_topics (last_index_demand_at DESC, id)
    WHERE kind = 'public_topic';

CREATE TABLE cn_index.public_bucket_reader_cursor (
    id BOOLEAN PRIMARY KEY DEFAULT TRUE CHECK (id),
    last_fair_topic TEXT NOT NULL DEFAULT ''
);
INSERT INTO cn_index.public_bucket_reader_cursor (id) VALUES (TRUE);
