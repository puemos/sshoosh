ALTER TABLE thread_reads ADD COLUMN unread_count INTEGER NOT NULL DEFAULT 0;
ALTER TABLE conversation_members ADD COLUMN unread_count INTEGER NOT NULL DEFAULT 0;

UPDATE thread_reads
SET unread_count = (
  SELECT COUNT(*)
  FROM comments cm
  WHERE cm.thread_id = thread_reads.thread_id
    AND cm.deleted_at IS NULL
    AND cm.obj_index > thread_reads.last_read_index
);

UPDATE conversation_members
SET unread_count = (
  SELECT COUNT(*)
  FROM conversation_messages msg
  WHERE msg.conversation_id = conversation_members.conversation_id
    AND msg.deleted_at IS NULL
    AND msg.obj_index > conversation_members.last_read_index
);

CREATE INDEX IF NOT EXISTS idx_conversation_members_account
  ON conversation_members(account_id, conversation_id);

CREATE INDEX IF NOT EXISTS idx_accounts_username_lower
  ON accounts(lower(username));

CREATE INDEX IF NOT EXISTS idx_threads_channel_visibility_activity
  ON threads(channel_id, deleted_at, archived_at, pinned_at, last_activity_at DESC, id DESC);

CREATE INDEX IF NOT EXISTS idx_comments_thread_unread
  ON comments(thread_id, obj_index ASC)
  WHERE deleted_at IS NULL;

CREATE INDEX IF NOT EXISTS idx_conversation_messages_unread
  ON conversation_messages(conversation_id, obj_index ASC)
  WHERE deleted_at IS NULL;
