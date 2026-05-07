use super::*;

const MIGRATION_INITIAL: &str = include_str!("../../migrations/20260430000000_initial.sql");
const MIGRATION_PENDING_USERNAME: &str =
    include_str!("../../migrations/20260430000001_pending_username.sql");
const MIGRATION_REMOTE_SECURITY: &str =
    include_str!("../../migrations/20260430000001_remote_security.sql");
const MIGRATION_SAVED_MESSAGES: &str =
    include_str!("../../migrations/20260501000000_saved_messages.sql");
const MIGRATION_NOTIFICATION_ARCHIVE: &str =
    include_str!("../../migrations/20260501000001_notification_archive.sql");
const MIGRATION_PERFORMANCE_COUNTERS: &str =
    include_str!("../../migrations/20260501000002_performance_counters.sql");
const MIGRATION_DM_SIDEBAR_SCALE: &str =
    include_str!("../../migrations/20260501000003_dm_sidebar_scale.sql");
const MIGRATION_DEVICE_LINK_TOKENS: &str =
    include_str!("../../migrations/20260501000004_device_link_tokens.sql");
const MIGRATION_MESSAGE_LABELS: &str =
    include_str!("../../migrations/20260501000005_message_labels.sql");
const MIGRATION_QUERY_PERFORMANCE: &str =
    include_str!("../../migrations/20260501000006_query_performance.sql");
const MIGRATION_USERNAME_RESERVATIONS: &str =
    include_str!("../../migrations/20260501000007_username_reservations.sql");

impl Database {
    pub async fn init(&self) -> anyhow::Result<()> {
        let _guard = self.write_lock.lock().await;
        self.execute_batch_unchecked(
            "CREATE TABLE IF NOT EXISTS _sshoosh_migrations (
               version TEXT PRIMARY KEY,
               applied_at TEXT NOT NULL
             );",
        )
        .await?;
        for (version, sql) in [
            ("20260430000000_initial", MIGRATION_INITIAL),
            (
                "20260430000001_pending_username",
                MIGRATION_PENDING_USERNAME,
            ),
            ("20260430000001_remote_security", MIGRATION_REMOTE_SECURITY),
            ("20260501000000_saved_messages", MIGRATION_SAVED_MESSAGES),
            (
                "20260501000001_notification_archive",
                MIGRATION_NOTIFICATION_ARCHIVE,
            ),
            (
                "20260501000002_performance_counters",
                MIGRATION_PERFORMANCE_COUNTERS,
            ),
            (
                "20260501000003_dm_sidebar_scale",
                MIGRATION_DM_SIDEBAR_SCALE,
            ),
            (
                "20260501000004_device_link_tokens",
                MIGRATION_DEVICE_LINK_TOKENS,
            ),
            ("20260501000005_message_labels", MIGRATION_MESSAGE_LABELS),
            (
                "20260501000006_query_performance",
                MIGRATION_QUERY_PERFORMANCE,
            ),
            (
                "20260501000007_username_reservations",
                MIGRATION_USERNAME_RESERVATIONS,
            ),
        ] {
            let exists: Option<String> =
                query_scalar("SELECT version FROM _sshoosh_migrations WHERE version = ?")
                    .bind(version)
                    .fetch_optional_unchecked(self)
                    .await?;
            if exists.is_some() {
                continue;
            }
            if version == "20260501000001_notification_archive"
                && self.notification_archive_column_exists().await?
            {
                self.execute_batch_unchecked(
                    "CREATE INDEX IF NOT EXISTS idx_notifications_account_archived
                       ON notifications(account_id, archived_at, created_at DESC);",
                )
                .await?;
                query("INSERT INTO _sshoosh_migrations (version, applied_at) VALUES (?, ?)")
                    .bind(version)
                    .bind(now())
                    .execute_unchecked(self)
                    .await?;
                continue;
            }
            if version == "20260501000002_performance_counters"
                && self.performance_counter_columns_exist().await?
            {
                query("INSERT INTO _sshoosh_migrations (version, applied_at) VALUES (?, ?)")
                    .bind(version)
                    .bind(now())
                    .execute_unchecked(self)
                    .await?;
                continue;
            }
            if version == "20260501000006_query_performance" {
                self.apply_query_performance_migration(sql).await?;
                query("INSERT INTO _sshoosh_migrations (version, applied_at) VALUES (?, ?)")
                    .bind(version)
                    .bind(now())
                    .execute_unchecked(self)
                    .await?;
                continue;
            }
            self.execute_batch_unchecked(sql).await?;
            query("INSERT INTO _sshoosh_migrations (version, applied_at) VALUES (?, ?)")
                .bind(version)
                .bind(now())
                .execute_unchecked(self)
                .await?;
        }
        self.backfill_message_labels_once().await?;
        self.validate_encryption(self.allow_plaintext_encryption_migration)
            .await?;
        Ok(())
    }

    async fn backfill_message_labels_once(&self) -> anyhow::Result<()> {
        let version = "20260501000005_message_labels_backfill";
        let exists: Option<String> =
            query_scalar("SELECT version FROM _sshoosh_migrations WHERE version = ?")
                .bind(version)
                .fetch_optional_unchecked(self)
                .await?;
        if exists.is_some() {
            return Ok(());
        }
        self.backfill_message_labels().await?;
        query("INSERT INTO _sshoosh_migrations (version, applied_at) VALUES (?, ?)")
            .bind(version)
            .bind(now())
            .execute_unchecked(self)
            .await?;
        Ok(())
    }

    async fn backfill_message_labels(&self) -> anyhow::Result<()> {
        let mut tx = self.transaction_unchecked().await?;
        let rows = query(
            "SELECT 'thread' AS source_kind,
                    t.id AS source_id,
                    t.channel_id,
                    t.id AS thread_id,
                    NULL AS conversation_id,
                    NULL AS obj_index,
                    t.title,
                    t.body,
                    t.created_at
             FROM threads t
             WHERE t.deleted_at IS NULL
             UNION ALL
             SELECT 'comment' AS source_kind,
                    cm.id AS source_id,
                    cm.channel_id,
                    cm.thread_id,
                    NULL AS conversation_id,
                    cm.obj_index,
                    '' AS title,
                    cm.body,
                    cm.created_at
             FROM comments cm
             JOIN threads t ON t.id = cm.thread_id
             WHERE cm.deleted_at IS NULL AND t.deleted_at IS NULL
             UNION ALL
             SELECT 'dm' AS source_kind,
                    dm.id AS source_id,
                    NULL AS channel_id,
                    NULL AS thread_id,
                    dm.conversation_id,
                    dm.obj_index,
                    '' AS title,
                    dm.body,
                    dm.created_at
             FROM conversation_messages dm
             WHERE dm.deleted_at IS NULL",
        )
        .fetch_all_unchecked(&mut tx)
        .await?;
        for row in rows {
            let source_kind: String = row.get("source_kind")?;
            let source_id: String = row.get("source_id")?;
            let channel_id: Option<String> = row.get("channel_id")?;
            let thread_id: Option<String> = row.get("thread_id")?;
            let conversation_id: Option<String> = row.get("conversation_id")?;
            let obj_index: Option<i64> = row.get("obj_index")?;
            let title: String = row.get("title")?;
            let body: String = row.get("body")?;
            let created_at: String = row.get("created_at")?;
            let text = if title.is_empty() {
                body
            } else {
                format!("{title}\n{body}")
            };
            for tag in parse_labels(&text) {
                query(
                    "INSERT OR IGNORE INTO message_labels
                     (tag, source_kind, source_id, channel_id, thread_id, conversation_id, obj_index, created_at)
                     VALUES (?, ?, ?, ?, ?, ?, ?, ?)",
                )
                .bind(tag)
                .bind(&source_kind)
                .bind(&source_id)
                .bind(channel_id.as_deref())
                .bind(thread_id.as_deref())
                .bind(conversation_id.as_deref())
                .bind(obj_index)
                .bind(&created_at)
                .execute_unchecked(&mut tx)
                .await?;
            }
        }
        tx.commit().await?;
        Ok(())
    }

    async fn notification_archive_column_exists(&self) -> anyhow::Result<bool> {
        let count: i64 = query_scalar(
            "SELECT COUNT(*)
             FROM pragma_table_info('notifications')
             WHERE name = 'archived_at'",
        )
        .fetch_one_unchecked(self)
        .await?;
        Ok(count > 0)
    }

    async fn performance_counter_columns_exist(&self) -> anyhow::Result<bool> {
        let thread_count: i64 = query_scalar(
            "SELECT COUNT(*)
             FROM pragma_table_info('thread_reads')
             WHERE name = 'unread_count'",
        )
        .fetch_one_unchecked(self)
        .await?;
        let conversation_count: i64 = query_scalar(
            "SELECT COUNT(*)
             FROM pragma_table_info('conversation_members')
             WHERE name = 'unread_count'",
        )
        .fetch_one_unchecked(self)
        .await?;
        Ok(thread_count > 0 && conversation_count > 0)
    }

    async fn threads_name_key_column_exists(&self) -> anyhow::Result<bool> {
        let count: i64 = query_scalar(
            "SELECT COUNT(*)
             FROM pragma_table_info('threads')
             WHERE name = 'name_key'",
        )
        .fetch_one_unchecked(self)
        .await?;
        Ok(count > 0)
    }

    async fn apply_query_performance_migration(&self, sql: &str) -> anyhow::Result<()> {
        if !self.threads_name_key_column_exists().await? {
            self.execute_batch_unchecked("ALTER TABLE threads ADD COLUMN name_key TEXT;")
                .await?;
        }
        self.backfill_thread_name_keys().await?;
        self.execute_batch_unchecked(sql).await?;
        Ok(())
    }

    async fn backfill_thread_name_keys(&self) -> anyhow::Result<()> {
        let mut tx = self.transaction_unchecked().await?;
        let rows = query(
            "SELECT id, title
             FROM threads
             WHERE name_key IS NULL OR name_key = ''",
        )
        .fetch_all_unchecked(&mut tx)
        .await?;
        for row in rows {
            let id: String = row.get("id")?;
            let title: String = row.get("title")?;
            query("UPDATE threads SET name_key = ? WHERE id = ?")
                .bind(normalize_name_key(&title))
                .bind(&id)
                .execute_unchecked(&mut tx)
                .await?;
        }
        tx.commit().await?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::super::*;
    use super::*;
    use uuid::Uuid;

    fn temp_path(name: &str) -> PathBuf {
        std::env::temp_dir().join(format!("sshoosh-db-{name}-{}", Uuid::now_v7()))
    }

    #[tokio::test]
    async fn query_performance_schema_exists_on_fresh_database() -> anyhow::Result<()> {
        let db = Database::connect(&temp_path("query-performance-fresh")).await?;
        db.init().await?;

        let name_key_columns: i64 = query_scalar(
            "SELECT COUNT(*)
             FROM pragma_table_info('threads')
             WHERE name = 'name_key'",
        )
        .fetch_one(db.read_pool())
        .await?;
        assert_eq!(name_key_columns, 1);

        let search_documents_tables: i64 = query_scalar(
            "SELECT COUNT(*)
             FROM sqlite_master
             WHERE type = 'table' AND name = 'search_documents'",
        )
        .fetch_one(db.read_pool())
        .await?;
        assert_eq!(search_documents_tables, 1);

        for index_name in [
            "idx_notifications_account_thread",
            "idx_mentions_target_thread",
            "idx_message_labels_source",
            "idx_saved_messages_account_kind_saved",
            "idx_threads_channel_name_key_active",
            "idx_ssh_keys_account_created",
        ] {
            let count: i64 = query_scalar(
                "SELECT COUNT(*)
                 FROM sqlite_master
                 WHERE type = 'index' AND name = ?",
            )
            .bind(index_name)
            .fetch_one(db.read_pool())
            .await?;
            assert_eq!(count, 1, "missing index {index_name}");
        }

        Ok(())
    }
    #[tokio::test]
    async fn query_performance_backfills_search_documents_and_thread_name_keys()
    -> anyhow::Result<()> {
        let db = Database::connect(&temp_path("query-performance-backfill")).await?;
        db.init().await?;
        let now = now();
        query(
            "INSERT INTO accounts
             (id, username, display_name, role, settings_json, created_at, updated_at, activated_at)
             VALUES ('owner', 'owner', 'owner', 'owner', '{}', ?, ?, ?)",
        )
        .bind(&now)
        .bind(&now)
        .bind(&now)
        .execute(db.write_pool())
        .await?;
        query(
            "INSERT INTO channels
             (id, slug, name, visibility, created_by_account_id, created_at, updated_at)
             VALUES ('general', 'general', 'general', 'public', 'owner', ?, ?)",
        )
        .bind(&now)
        .bind(&now)
        .execute(db.write_pool())
        .await?;
        query(
            "INSERT INTO threads
             (id, channel_id, creator_account_id, title, body, last_activity_at, created_at, updated_at)
             VALUES ('thread-1', 'general', 'owner', 'Release Plan', 'body', ?, ?, ?)",
        )
        .bind(&now)
        .bind(&now)
        .bind(&now)
        .execute(db.write_pool())
        .await?;
        query(
            "INSERT INTO search_index
             (kind, object_id, channel_id, thread_id, conversation_id, title, body, context)
             VALUES ('thread', 'thread-1', 'general', 'thread-1', NULL, 'Release Plan', 'body', '#general')",
        )
        .execute(db.write_pool())
        .await?;
        query("DELETE FROM search_documents")
            .execute(db.write_pool())
            .await?;
        query("UPDATE threads SET name_key = NULL WHERE id = 'thread-1'")
            .execute(db.write_pool())
            .await?;

        db.apply_query_performance_migration(MIGRATION_QUERY_PERFORMANCE)
            .await?;

        let mapped_rows: i64 = query_scalar(
            "SELECT COUNT(*)
             FROM search_documents
             WHERE kind = 'thread' AND object_id = 'thread-1'",
        )
        .fetch_one(db.read_pool())
        .await?;
        assert_eq!(mapped_rows, 1);

        let name_key: String = query_scalar("SELECT name_key FROM threads WHERE id = 'thread-1'")
            .fetch_one(db.read_pool())
            .await?;
        assert_eq!(name_key, "release-plan");

        Ok(())
    }
}
