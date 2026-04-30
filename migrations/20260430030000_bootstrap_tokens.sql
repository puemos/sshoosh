CREATE TABLE IF NOT EXISTS bootstrap_tokens (
  id TEXT PRIMARY KEY,
  code_hash TEXT NOT NULL UNIQUE,
  created_at TEXT NOT NULL,
  used_by_account_id TEXT REFERENCES accounts(id),
  used_at TEXT
);
