CREATE INDEX IF NOT EXISTS idx_conversations_archived_activity
  ON conversations(archived_at, last_activity_at DESC, id DESC);

CREATE INDEX IF NOT EXISTS idx_presence_sessions_connected_last_seen
  ON presence_sessions(last_seen_at)
  WHERE disconnected_at IS NULL;
