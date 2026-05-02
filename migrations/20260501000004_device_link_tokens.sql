CREATE TABLE IF NOT EXISTS device_link_tokens (
  id TEXT PRIMARY KEY,
  account_id TEXT NOT NULL REFERENCES accounts(id) ON DELETE CASCADE,
  code_hash TEXT NOT NULL UNIQUE,
  label TEXT,
  created_at TEXT NOT NULL,
  expires_at TEXT NOT NULL,
  used_at TEXT,
  used_by_key_id TEXT REFERENCES ssh_keys(id)
);

CREATE INDEX IF NOT EXISTS idx_device_link_tokens_account
  ON device_link_tokens(account_id, created_at DESC);
