CREATE TABLE IF NOT EXISTS account_username_reservations (
  normalized_username TEXT PRIMARY KEY,
  username TEXT NOT NULL,
  account_id TEXT NOT NULL REFERENCES accounts(id),
  first_used_at TEXT NOT NULL,
  last_used_at TEXT,
  current INTEGER NOT NULL DEFAULT 0 CHECK (current IN (0, 1))
);

CREATE INDEX IF NOT EXISTS idx_account_username_reservations_account
  ON account_username_reservations(account_id, first_used_at);

INSERT OR IGNORE INTO account_username_reservations
  (normalized_username, username, account_id, first_used_at, last_used_at, current)
SELECT lower(username), username, id, COALESCE(activated_at, created_at), NULL, 1
FROM accounts
WHERE activated_at IS NOT NULL;
