CREATE TABLE IF NOT EXISTS search_documents (
  rowid INTEGER PRIMARY KEY,
  kind TEXT NOT NULL CHECK (kind IN ('thread', 'comment', 'dm')),
  object_id TEXT NOT NULL,
  channel_id TEXT REFERENCES channels(id) ON DELETE CASCADE,
  thread_id TEXT REFERENCES threads(id) ON DELETE CASCADE,
  conversation_id TEXT REFERENCES conversations(id) ON DELETE CASCADE,
  UNIQUE(kind, object_id)
);

INSERT OR IGNORE INTO search_documents
  (rowid, kind, object_id, channel_id, thread_id, conversation_id)
SELECT rowid, kind, object_id, channel_id, thread_id, conversation_id
FROM search_index
WHERE object_id IS NOT NULL;

CREATE INDEX IF NOT EXISTS idx_message_labels_source
  ON message_labels(source_kind, source_id);

CREATE INDEX IF NOT EXISTS idx_message_labels_tag_kind_created
  ON message_labels(tag, source_kind, created_at DESC, source_id DESC);

CREATE INDEX IF NOT EXISTS idx_saved_messages_account_kind_saved
  ON saved_messages(account_id, source_kind, saved_at DESC, source_id DESC);

CREATE INDEX IF NOT EXISTS idx_notifications_account_archived_created_id
  ON notifications(account_id, archived_at, created_at DESC, id DESC);

CREATE INDEX IF NOT EXISTS idx_notifications_account_thread
  ON notifications(account_id, thread_id, archived_at, read_at, created_at DESC, id DESC);

CREATE INDEX IF NOT EXISTS idx_notifications_account_conversation
  ON notifications(account_id, conversation_id, archived_at, read_at, created_at DESC, id DESC);

CREATE INDEX IF NOT EXISTS idx_mentions_target_created_id
  ON mentions(target_account_id, created_at DESC, id DESC);

CREATE INDEX IF NOT EXISTS idx_mentions_target_thread
  ON mentions(target_account_id, thread_id, read_at, created_at DESC, id DESC);

CREATE INDEX IF NOT EXISTS idx_mentions_target_conversation
  ON mentions(target_account_id, conversation_id, read_at, created_at DESC, id DESC);

CREATE INDEX IF NOT EXISTS idx_ssh_keys_account_created
  ON ssh_keys(account_id, created_at, id);

CREATE INDEX IF NOT EXISTS idx_invites_created_id
  ON invites(created_at DESC, id DESC);

CREATE INDEX IF NOT EXISTS idx_audit_log_created_id
  ON audit_log(created_at DESC, id DESC);

CREATE INDEX IF NOT EXISTS idx_accounts_username_lower_id
  ON accounts(lower(username), id);

CREATE INDEX IF NOT EXISTS idx_threads_channel_list_order
  ON threads(channel_id, deleted_at, (pinned_at IS NULL), pinned_at DESC, last_activity_at DESC, id DESC);

CREATE INDEX IF NOT EXISTS idx_threads_global_active
  ON threads(deleted_at, archived_at, channel_id, id);

CREATE INDEX IF NOT EXISTS idx_threads_channel_name_key_active
  ON threads(channel_id, name_key)
  WHERE deleted_at IS NULL AND archived_at IS NULL;

CREATE INDEX IF NOT EXISTS idx_threads_name_key_active
  ON threads(name_key)
  WHERE deleted_at IS NULL AND archived_at IS NULL;

CREATE INDEX IF NOT EXISTS idx_comments_thread_author_active
  ON comments(thread_id, author_account_id)
  WHERE deleted_at IS NULL;
