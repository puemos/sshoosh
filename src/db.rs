use std::{path::Path, str::FromStr};

use anyhow::Context;
use sqlx::{
    Executor, SqlitePool,
    sqlite::{SqliteConnectOptions, SqliteJournalMode, SqlitePoolOptions, SqliteSynchronous},
};

#[derive(Clone)]
pub struct Database {
    read_pool: SqlitePool,
    write_pool: SqlitePool,
}

impl Database {
    pub async fn connect(path: &Path) -> anyhow::Result<Self> {
        if let Some(parent) = path.parent().filter(|p| !p.as_os_str().is_empty()) {
            std::fs::create_dir_all(parent)
                .with_context(|| format!("creating {}", parent.display()))?;
        }

        let uri = format!("sqlite://{}", path.display());
        let opts = SqliteConnectOptions::from_str(&uri)?
            .create_if_missing(true)
            .journal_mode(SqliteJournalMode::Wal)
            .synchronous(SqliteSynchronous::Normal)
            .pragma("foreign_keys", "ON")
            .pragma("busy_timeout", "5000")
            .pragma("temp_store", "MEMORY");

        let read_pool = SqlitePoolOptions::new()
            .max_connections(8)
            .connect_with(opts.clone())
            .await?;
        let write_pool = SqlitePoolOptions::new()
            .max_connections(1)
            .connect_with(opts)
            .await?;

        Ok(Self {
            read_pool,
            write_pool,
        })
    }

    pub fn read_pool(&self) -> &SqlitePool {
        &self.read_pool
    }

    pub fn write_pool(&self) -> &SqlitePool {
        &self.write_pool
    }

    pub async fn init(&self) -> anyhow::Result<()> {
        for statement in SCHEMA.split(';') {
            let statement = statement.trim();
            if statement.is_empty() {
                continue;
            }
            sqlx::query(statement).execute(self.write_pool()).await?;
        }

        Ok(())
    }

    pub async fn doctor(&self) -> anyhow::Result<()> {
        let row: (String,) = sqlx::query_as("PRAGMA integrity_check")
            .fetch_one(self.read_pool())
            .await?;
        anyhow::ensure!(row.0 == "ok", "sqlite integrity_check failed: {}", row.0);
        Ok(())
    }

    pub async fn backup_to(&self, out: &str) -> anyhow::Result<()> {
        let escaped = out.replace('\'', "''");
        let sql = format!("VACUUM INTO '{escaped}'");
        self.write_pool().execute(sql.as_str()).await?;
        Ok(())
    }
}

const SCHEMA: &str = r#"
CREATE TABLE IF NOT EXISTS accounts (
  id TEXT PRIMARY KEY,
  username TEXT NOT NULL UNIQUE,
  display_name TEXT NOT NULL,
  role TEXT NOT NULL CHECK (role IN ('owner', 'admin', 'member')),
  settings_json TEXT NOT NULL DEFAULT '{}',
  created_at TEXT NOT NULL,
  updated_at TEXT NOT NULL,
  last_seen_at TEXT,
  activated_at TEXT,
  disabled_at TEXT
);

CREATE TABLE IF NOT EXISTS ssh_keys (
  id TEXT PRIMARY KEY,
  account_id TEXT NOT NULL REFERENCES accounts(id) ON DELETE CASCADE,
  fingerprint TEXT NOT NULL UNIQUE,
  public_key TEXT NOT NULL,
  label TEXT,
  created_at TEXT NOT NULL,
  last_used_at TEXT,
  revoked_at TEXT
);

CREATE TABLE IF NOT EXISTS invites (
  id TEXT PRIMARY KEY,
  code_hash TEXT NOT NULL UNIQUE,
  role_on_accept TEXT NOT NULL CHECK (role_on_accept IN ('admin', 'member')),
  created_by_account_id TEXT NOT NULL REFERENCES accounts(id),
  accepted_by_account_id TEXT REFERENCES accounts(id),
  created_at TEXT NOT NULL,
  expires_at TEXT,
  revoked_at TEXT,
  accepted_at TEXT
);

CREATE TABLE IF NOT EXISTS channels (
  id TEXT PRIMARY KEY,
  slug TEXT NOT NULL UNIQUE,
  name TEXT NOT NULL,
  visibility TEXT NOT NULL CHECK (visibility IN ('public', 'private')),
  topic TEXT,
  created_by_account_id TEXT NOT NULL REFERENCES accounts(id),
  created_at TEXT NOT NULL,
  updated_at TEXT NOT NULL,
  archived_at TEXT
);

CREATE TABLE IF NOT EXISTS channel_members (
  channel_id TEXT NOT NULL REFERENCES channels(id) ON DELETE CASCADE,
  account_id TEXT NOT NULL REFERENCES accounts(id) ON DELETE CASCADE,
  role TEXT NOT NULL DEFAULT 'member',
  joined_at TEXT NOT NULL,
  PRIMARY KEY (channel_id, account_id)
);

CREATE TABLE IF NOT EXISTS threads (
  id TEXT PRIMARY KEY,
  channel_id TEXT NOT NULL REFERENCES channels(id) ON DELETE CASCADE,
  creator_account_id TEXT NOT NULL REFERENCES accounts(id),
  title TEXT NOT NULL,
  body TEXT NOT NULL,
  comment_count INTEGER NOT NULL DEFAULT 0,
  last_comment_index INTEGER NOT NULL DEFAULT 0,
  last_activity_at TEXT NOT NULL,
  created_at TEXT NOT NULL,
  updated_at TEXT NOT NULL,
  edited_at TEXT,
  archived_at TEXT,
  pinned_at TEXT,
  deleted_at TEXT
);

CREATE INDEX IF NOT EXISTS idx_threads_channel_activity
ON threads(channel_id, last_activity_at DESC, id DESC);

CREATE TABLE IF NOT EXISTS comments (
  id TEXT PRIMARY KEY,
  thread_id TEXT NOT NULL REFERENCES threads(id) ON DELETE CASCADE,
  channel_id TEXT NOT NULL REFERENCES channels(id) ON DELETE CASCADE,
  author_account_id TEXT NOT NULL REFERENCES accounts(id),
  obj_index INTEGER NOT NULL,
  body TEXT NOT NULL,
  created_at TEXT NOT NULL,
  updated_at TEXT NOT NULL,
  edited_at TEXT,
  deleted_at TEXT,
  UNIQUE(thread_id, obj_index)
);

CREATE INDEX IF NOT EXISTS idx_comments_thread_index
ON comments(thread_id, obj_index ASC);

CREATE TABLE IF NOT EXISTS thread_reads (
  thread_id TEXT NOT NULL REFERENCES threads(id) ON DELETE CASCADE,
  account_id TEXT NOT NULL REFERENCES accounts(id) ON DELETE CASCADE,
  last_read_index INTEGER NOT NULL DEFAULT 0,
  marked_unread_at TEXT,
  muted_until TEXT,
  saved_at TEXT,
  PRIMARY KEY (thread_id, account_id)
);

CREATE TABLE IF NOT EXISTS conversations (
  id TEXT PRIMARY KEY,
  dm_key TEXT NOT NULL UNIQUE,
  creator_account_id TEXT NOT NULL REFERENCES accounts(id),
  last_message_index INTEGER NOT NULL DEFAULT 0,
  last_activity_at TEXT NOT NULL,
  created_at TEXT NOT NULL,
  archived_at TEXT
);

CREATE TABLE IF NOT EXISTS conversation_members (
  conversation_id TEXT NOT NULL REFERENCES conversations(id) ON DELETE CASCADE,
  account_id TEXT NOT NULL REFERENCES accounts(id) ON DELETE CASCADE,
  joined_at TEXT NOT NULL,
  last_read_index INTEGER NOT NULL DEFAULT 0,
  muted_until TEXT,
  PRIMARY KEY (conversation_id, account_id)
);

CREATE TABLE IF NOT EXISTS conversation_messages (
  id TEXT PRIMARY KEY,
  conversation_id TEXT NOT NULL REFERENCES conversations(id) ON DELETE CASCADE,
  author_account_id TEXT NOT NULL REFERENCES accounts(id),
  obj_index INTEGER NOT NULL,
  body TEXT NOT NULL,
  created_at TEXT NOT NULL,
  updated_at TEXT NOT NULL,
  edited_at TEXT,
  deleted_at TEXT,
  UNIQUE(conversation_id, obj_index)
);

CREATE INDEX IF NOT EXISTS idx_conversation_messages_index
ON conversation_messages(conversation_id, obj_index ASC);

CREATE TABLE IF NOT EXISTS event_log (
  seq INTEGER PRIMARY KEY AUTOINCREMENT,
  created_at TEXT NOT NULL,
  channel_id TEXT,
  thread_id TEXT,
  conversation_id TEXT,
  kind TEXT NOT NULL,
  payload_json TEXT NOT NULL
);

CREATE INDEX IF NOT EXISTS idx_event_log_seq ON event_log(seq);
CREATE INDEX IF NOT EXISTS idx_event_log_channel_seq ON event_log(channel_id, seq);
CREATE INDEX IF NOT EXISTS idx_event_log_thread_seq ON event_log(thread_id, seq);
CREATE INDEX IF NOT EXISTS idx_event_log_conversation_seq ON event_log(conversation_id, seq);

CREATE TABLE IF NOT EXISTS audit_log (
  id TEXT PRIMARY KEY,
  actor_account_id TEXT REFERENCES accounts(id),
  action TEXT NOT NULL,
  target TEXT,
  metadata_json TEXT NOT NULL DEFAULT '{}',
  created_at TEXT NOT NULL
);
"#;
