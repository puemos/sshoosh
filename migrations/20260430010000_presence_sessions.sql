CREATE TABLE presence_sessions (
  id TEXT PRIMARY KEY,
  account_id TEXT NOT NULL REFERENCES accounts(id) ON DELETE CASCADE,
  started_at TEXT NOT NULL,
  last_seen_at TEXT NOT NULL,
  disconnected_at TEXT
);

CREATE INDEX idx_presence_sessions_account_active
  ON presence_sessions(account_id, disconnected_at, last_seen_at);
