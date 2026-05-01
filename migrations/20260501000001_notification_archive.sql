ALTER TABLE notifications ADD COLUMN archived_at TEXT;

CREATE INDEX idx_notifications_account_archived
  ON notifications(account_id, archived_at, created_at DESC);
