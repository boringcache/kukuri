CREATE INDEX IF NOT EXISTS idx_dm_outbox_peer_cursor
    ON dm_outbox(peer_pubkey, created_at ASC, message_id ASC, dm_id ASC);
