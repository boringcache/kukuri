DROP INDEX IF EXISTS idx_notifications_dispatch_seq;
DROP TRIGGER IF EXISTS notifications_assign_dispatch_seq;
DROP TABLE IF EXISTS notification_dispatch_clock;
ALTER TABLE notifications DROP COLUMN dispatch_seq;
