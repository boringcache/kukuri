CREATE TABLE blob_objects (
    blob_hash TEXT PRIMARY KEY,
    status TEXT NOT NULL
);
DROP INDEX remote_adult_media_hash_refs_hash;
DROP TABLE remote_adult_media_hash_refs;
ALTER TABLE adult_media_hashes DROP COLUMN is_protected;
DROP INDEX remote_content_cache_protected_ref_owner;
DROP TABLE remote_content_cache_protected_ref;
DROP TABLE remote_content_cache_usage;
DROP INDEX remote_content_cache_record;
DROP INDEX remote_content_cache_lru;
DROP TABLE remote_content_cache;
