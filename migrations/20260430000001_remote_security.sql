CREATE TABLE IF NOT EXISTS server_leases (
  name TEXT PRIMARY KEY,
  node_id TEXT NOT NULL,
  fencing_token INTEGER NOT NULL,
  lease_until TEXT NOT NULL,
  heartbeat_at TEXT NOT NULL
);

ALTER TABLE presence_sessions ADD COLUMN node_id TEXT;

CREATE TABLE IF NOT EXISTS webhook_jobs (
  id TEXT PRIMARY KEY,
  payload_json TEXT NOT NULL,
  created_at TEXT NOT NULL,
  delivered_at TEXT,
  failed_at TEXT,
  claimed_by_node_id TEXT,
  claimed_until TEXT,
  claim_token INTEGER
);

CREATE INDEX IF NOT EXISTS idx_webhook_jobs_claim
  ON webhook_jobs(delivered_at, failed_at, claimed_until, created_at);
