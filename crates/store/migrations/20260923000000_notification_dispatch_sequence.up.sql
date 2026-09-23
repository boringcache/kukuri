-- Only new inserts enter OS dispatch. Existing inbox rows remain available to the UI
-- but do not become a toast backlog when this migration is installed.
ALTER TABLE notifications ADD COLUMN dispatch_seq INTEGER;

CREATE TABLE notification_dispatch_clock (
    singleton INTEGER PRIMARY KEY,
    last_seq INTEGER NOT NULL
);
INSERT INTO notification_dispatch_clock (singleton, last_seq) VALUES (1, 0);

CREATE TRIGGER notifications_assign_dispatch_seq
AFTER INSERT ON notifications
BEGIN
    UPDATE notification_dispatch_clock SET last_seq = last_seq + 1 WHERE singleton = 1;
    UPDATE notifications
    SET dispatch_seq = (SELECT last_seq FROM notification_dispatch_clock WHERE singleton = 1)
    WHERE notification_id = NEW.notification_id;
END;

CREATE INDEX idx_notifications_dispatch_seq
ON notifications(dispatch_seq)
WHERE dispatch_seq IS NOT NULL;
