CREATE TABLE IF NOT EXISTS message_labels (
  tag TEXT NOT NULL,
  source_kind TEXT NOT NULL CHECK (source_kind IN ('thread', 'comment', 'dm')),
  source_id TEXT NOT NULL,
  channel_id TEXT REFERENCES channels(id) ON DELETE CASCADE,
  thread_id TEXT REFERENCES threads(id) ON DELETE CASCADE,
  conversation_id TEXT REFERENCES conversations(id) ON DELETE CASCADE,
  obj_index INTEGER,
  created_at TEXT NOT NULL,
  PRIMARY KEY (tag, source_kind, source_id)
);

CREATE INDEX IF NOT EXISTS idx_message_labels_tag_created
  ON message_labels(tag, created_at DESC, source_id DESC);

CREATE INDEX IF NOT EXISTS idx_message_labels_thread
  ON message_labels(thread_id);

CREATE INDEX IF NOT EXISTS idx_message_labels_conversation
  ON message_labels(conversation_id);
