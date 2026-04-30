CREATE TABLE accounts (
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

CREATE TABLE ssh_keys (
  id TEXT PRIMARY KEY,
  account_id TEXT NOT NULL REFERENCES accounts(id) ON DELETE CASCADE,
  fingerprint TEXT NOT NULL UNIQUE,
  public_key TEXT NOT NULL,
  label TEXT,
  created_at TEXT NOT NULL,
  last_used_at TEXT,
  revoked_at TEXT
);

CREATE TABLE invites (
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

CREATE TABLE bootstrap_tokens (
  id TEXT PRIMARY KEY,
  code_hash TEXT NOT NULL UNIQUE,
  created_at TEXT NOT NULL,
  used_by_account_id TEXT REFERENCES accounts(id),
  used_at TEXT
);

CREATE TABLE presence_sessions (
  id TEXT PRIMARY KEY,
  account_id TEXT NOT NULL REFERENCES accounts(id) ON DELETE CASCADE,
  started_at TEXT NOT NULL,
  last_seen_at TEXT NOT NULL,
  disconnected_at TEXT
);

CREATE INDEX idx_presence_sessions_account_active
  ON presence_sessions(account_id, disconnected_at, last_seen_at);

CREATE TABLE channels (
  id TEXT PRIMARY KEY,
  slug TEXT NOT NULL UNIQUE,
  name TEXT NOT NULL,
  visibility TEXT NOT NULL CHECK (visibility IN ('public', 'private')),
  topic TEXT,
  created_by_account_id TEXT NOT NULL REFERENCES accounts(id),
  created_at TEXT NOT NULL,
  updated_at TEXT NOT NULL,
  archived_at TEXT,
  archived_by_account_id TEXT REFERENCES accounts(id)
);

CREATE TABLE channel_members (
  channel_id TEXT NOT NULL REFERENCES channels(id) ON DELETE CASCADE,
  account_id TEXT NOT NULL REFERENCES accounts(id) ON DELETE CASCADE,
  role TEXT NOT NULL DEFAULT 'member',
  joined_at TEXT NOT NULL,
  PRIMARY KEY (channel_id, account_id)
);

CREATE INDEX idx_channel_members_account ON channel_members(account_id, channel_id);

CREATE TABLE threads (
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

CREATE INDEX idx_threads_channel_activity ON threads(channel_id, last_activity_at DESC, id DESC);

CREATE TABLE comments (
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

CREATE INDEX idx_comments_thread_index ON comments(thread_id, obj_index ASC);

CREATE TABLE thread_reads (
  thread_id TEXT NOT NULL REFERENCES threads(id) ON DELETE CASCADE,
  account_id TEXT NOT NULL REFERENCES accounts(id) ON DELETE CASCADE,
  last_read_index INTEGER NOT NULL DEFAULT 0,
  marked_unread_at TEXT,
  muted_until TEXT,
  saved_at TEXT,
  PRIMARY KEY (thread_id, account_id)
);

CREATE TABLE conversations (
  id TEXT PRIMARY KEY,
  dm_key TEXT NOT NULL UNIQUE,
  creator_account_id TEXT NOT NULL REFERENCES accounts(id),
  last_message_index INTEGER NOT NULL DEFAULT 0,
  last_activity_at TEXT NOT NULL,
  created_at TEXT NOT NULL,
  archived_at TEXT
);

CREATE TABLE conversation_members (
  conversation_id TEXT NOT NULL REFERENCES conversations(id) ON DELETE CASCADE,
  account_id TEXT NOT NULL REFERENCES accounts(id) ON DELETE CASCADE,
  joined_at TEXT NOT NULL,
  last_read_index INTEGER NOT NULL DEFAULT 0,
  muted_until TEXT,
  saved_at TEXT,
  PRIMARY KEY (conversation_id, account_id)
);

CREATE TABLE conversation_messages (
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

CREATE INDEX idx_conversation_messages_index ON conversation_messages(conversation_id, obj_index ASC);

CREATE TABLE mentions (
  id TEXT PRIMARY KEY,
  target_account_id TEXT NOT NULL REFERENCES accounts(id) ON DELETE CASCADE,
  actor_account_id TEXT NOT NULL REFERENCES accounts(id) ON DELETE CASCADE,
  source_kind TEXT NOT NULL CHECK (source_kind IN ('thread', 'comment', 'dm')),
  source_id TEXT NOT NULL,
  channel_id TEXT REFERENCES channels(id) ON DELETE CASCADE,
  thread_id TEXT REFERENCES threads(id) ON DELETE CASCADE,
  conversation_id TEXT REFERENCES conversations(id) ON DELETE CASCADE,
  obj_index INTEGER,
  created_at TEXT NOT NULL,
  read_at TEXT
);

CREATE INDEX idx_mentions_target ON mentions(target_account_id, read_at, created_at DESC);

CREATE TABLE reactions (
  id TEXT PRIMARY KEY,
  source_kind TEXT NOT NULL CHECK (source_kind IN ('thread', 'comment', 'dm')),
  source_id TEXT NOT NULL,
  account_id TEXT NOT NULL REFERENCES accounts(id) ON DELETE CASCADE,
  emoji TEXT NOT NULL,
  created_at TEXT NOT NULL,
  UNIQUE(source_kind, source_id, account_id, emoji)
);

CREATE INDEX idx_reactions_source ON reactions(source_kind, source_id);

CREATE TABLE notifications (
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

CREATE INDEX idx_notifications_account ON notifications(account_id, read_at, created_at DESC);

CREATE TABLE event_log (
  seq INTEGER PRIMARY KEY AUTOINCREMENT,
  created_at TEXT NOT NULL,
  channel_id TEXT,
  thread_id TEXT,
  conversation_id TEXT,
  kind TEXT NOT NULL,
  payload_json TEXT NOT NULL
);

CREATE INDEX idx_event_log_seq ON event_log(seq);
CREATE INDEX idx_event_log_channel_seq ON event_log(channel_id, seq);
CREATE INDEX idx_event_log_thread_seq ON event_log(thread_id, seq);
CREATE INDEX idx_event_log_conversation_seq ON event_log(conversation_id, seq);

CREATE TABLE audit_log (
  id TEXT PRIMARY KEY,
  actor_account_id TEXT REFERENCES accounts(id),
  action TEXT NOT NULL,
  target TEXT,
  metadata_json TEXT NOT NULL DEFAULT '{}',
  created_at TEXT NOT NULL
);

CREATE INDEX idx_audit_log_created ON audit_log(created_at DESC);

CREATE VIRTUAL TABLE search_index USING fts5(
  kind,
  object_id UNINDEXED,
  channel_id UNINDEXED,
  thread_id UNINDEXED,
  conversation_id UNINDEXED,
  title,
  body,
  context
);
