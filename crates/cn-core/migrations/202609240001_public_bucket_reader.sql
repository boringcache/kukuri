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

-- A verified withdrawal is durable across provider changes. A stale provider cannot reinsert it.
CREATE TABLE cn_index.known_post_withdrawals (
    scope_kind TEXT NOT NULL,
    scope_id TEXT NOT NULL,
    object_id TEXT NOT NULL,
    PRIMARY KEY (scope_kind, scope_id, object_id)
);

CREATE FUNCTION cn_index.reject_known_withdrawn_entry() RETURNS trigger LANGUAGE plpgsql AS $$
BEGIN
    PERFORM pg_advisory_xact_lock(hashtextextended(
        json_build_array(NEW.scope_kind, NEW.scope_id, NEW.object_id)::text, 0
    ));
    IF EXISTS (
        SELECT 1 FROM cn_index.known_post_withdrawals
        WHERE scope_kind = NEW.scope_kind AND scope_id = NEW.scope_id AND object_id = NEW.object_id
    ) THEN
        RAISE EXCEPTION 'known withdrawn post cannot be indexed';
    END IF;
    RETURN NEW;
END
$$;
CREATE TRIGGER reject_known_withdrawn_entry
    BEFORE INSERT OR UPDATE ON cn_index.index_entries
    FOR EACH ROW EXECUTE FUNCTION cn_index.reject_known_withdrawn_entry();

CREATE FUNCTION cn_index.apply_verified_withdrawal() RETURNS trigger LANGUAGE plpgsql AS $$
BEGIN
    PERFORM pg_advisory_xact_lock(hashtextextended(
        json_build_array(NEW.scope_kind, NEW.scope_id, NEW.object_id)::text, 0
    ));
    DELETE FROM cn_index.index_entries
    WHERE scope_kind = NEW.scope_kind AND scope_id = NEW.scope_id AND object_id = NEW.object_id;
    RETURN NEW;
END
$$;
CREATE TRIGGER apply_verified_withdrawal
    BEFORE INSERT ON cn_index.known_post_withdrawals
    FOR EACH ROW EXECUTE FUNCTION cn_index.apply_verified_withdrawal();
