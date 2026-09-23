ALTER TABLE game_room_cache
    DROP COLUMN score_revision;

ALTER TABLE live_session_cache
    DROP COLUMN revision;
