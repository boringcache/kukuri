ALTER TABLE live_session_cache
    ADD COLUMN revision INTEGER NOT NULL DEFAULT 0;

ALTER TABLE game_room_cache
    ADD COLUMN score_revision INTEGER;
