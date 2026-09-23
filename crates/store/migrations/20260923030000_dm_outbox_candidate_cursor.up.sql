CREATE INDEX IF NOT EXISTS idx_dm_outbox_candidate_cursor
ON dm_outbox(created_at, message_id, dm_id);
