DROP TABLE IF EXISTS webhook_jobs;
DROP TABLE IF EXISTS webhook_subscriptions;

CREATE TABLE notifications_new (
  id TEXT PRIMARY KEY,
  account_id TEXT NOT NULL REFERENCES accounts(id) ON DELETE CASCADE,
  actor_account_id TEXT REFERENCES accounts(id) ON DELETE SET NULL,
  kind TEXT NOT NULL CHECK (kind IN ('mention', 'dm', 'reply')),
  source_kind TEXT,
  source_id TEXT,
  channel_id TEXT REFERENCES channels(id) ON DELETE CASCADE,
  thread_id TEXT REFERENCES threads(id) ON DELETE CASCADE,
  conversation_id TEXT REFERENCES conversations(id) ON DELETE CASCADE,
  title TEXT NOT NULL,
  body TEXT NOT NULL,
  created_at TEXT NOT NULL,
  read_at TEXT
);

INSERT INTO notifications_new (
  id,
  account_id,
  actor_account_id,
  kind,
  source_kind,
  source_id,
  channel_id,
  thread_id,
  conversation_id,
  title,
  body,
  created_at,
  read_at
)
SELECT
  id,
  account_id,
  actor_account_id,
  kind,
  source_kind,
  source_id,
  channel_id,
  thread_id,
  conversation_id,
  title,
  body,
  created_at,
  read_at
FROM notifications
WHERE kind IN ('mention', 'dm', 'reply');

DROP TABLE notifications;
ALTER TABLE notifications_new RENAME TO notifications;

CREATE INDEX idx_notifications_account ON notifications(account_id, read_at, created_at DESC);
