CREATE TABLE remote_content_cache (
    kind TEXT NOT NULL,
    cache_key TEXT NOT NULL,
    scope_key TEXT NOT NULL,
    record_key TEXT,
    record_author TEXT,
    payload BLOB,
    charged_bytes INTEGER NOT NULL CHECK (charged_bytes >= 0),
    is_protected INTEGER NOT NULL DEFAULT 0 CHECK (is_protected IN (0, 1)),
    last_used_at INTEGER NOT NULL,
    PRIMARY KEY (kind, cache_key)
);

CREATE INDEX remote_content_cache_lru
    ON remote_content_cache(is_protected, last_used_at, kind, cache_key);
CREATE INDEX remote_content_cache_record
    ON remote_content_cache(scope_key, record_key, record_author)
    WHERE kind = 'record';

CREATE TABLE remote_content_cache_usage (
    id INTEGER PRIMARY KEY CHECK (id = 1),
    used_bytes INTEGER NOT NULL CHECK (used_bytes >= 0)
);
INSERT INTO remote_content_cache_usage (id, used_bytes) VALUES (1, 0);

CREATE TABLE remote_content_cache_protected_ref (
    kind TEXT NOT NULL,
    cache_key TEXT NOT NULL,
    ref_id TEXT NOT NULL,
    PRIMARY KEY (kind, cache_key, ref_id)
);
CREATE INDEX remote_content_cache_protected_ref_owner
    ON remote_content_cache_protected_ref(ref_id, kind, cache_key);
