CREATE INDEX IF NOT EXISTS idx_dm_outbox_never_attempted
ON dm_outbox(created_at, message_id, dm_id, peer_pubkey)
WHERE last_attempt_at IS NULL;

CREATE INDEX IF NOT EXISTS idx_dm_outbox_attempted_due
ON dm_outbox(last_attempt_at, created_at, message_id, dm_id, peer_pubkey)
WHERE last_attempt_at IS NOT NULL;
