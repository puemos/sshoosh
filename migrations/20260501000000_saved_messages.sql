CREATE TABLE saved_messages (
  account_id TEXT NOT NULL REFERENCES accounts(id) ON DELETE CASCADE,
  source_kind TEXT NOT NULL CHECK (source_kind IN ('comment', 'dm')),
  source_id TEXT NOT NULL,
  saved_at TEXT NOT NULL,
  PRIMARY KEY (account_id, source_kind, source_id)
);

CREATE INDEX idx_saved_messages_account_saved
  ON saved_messages(account_id, saved_at DESC);
