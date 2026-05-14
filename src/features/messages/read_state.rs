use super::*;

impl ServerState {
    pub async fn mark_thread_read(&self, account_id: &str, thread_id: &str) -> anyhow::Result<()> {
        let mut tx = begin(self.db.write_pool()).await?;
        let thread = load_thread_meta_tx(&mut tx, thread_id).await?;
        ensure_can_view_channel(&mut tx, account_id, &thread.channel_id).await?;
        let last_index: i64 = query_scalar("SELECT last_comment_index FROM threads WHERE id = ?")
            .bind(thread_id)
            .fetch_one(&mut tx)
            .await?;
        query(
            "INSERT INTO thread_reads (thread_id, account_id, last_read_index, unread_count, marked_unread_at)
             VALUES (?, ?, ?, 0, NULL)
             ON CONFLICT(thread_id, account_id)
             DO UPDATE SET last_read_index = excluded.last_read_index,
                           unread_count = 0,
                           marked_unread_at = NULL",
        )
        .bind(thread_id)
        .bind(account_id)
        .bind(last_index)
        .execute(&mut tx)
        .await?;
        let now = now();
        let notification_sql = format!(
            "UPDATE notifications SET read_at = COALESCE(read_at, ?)
             WHERE account_id = ? AND thread_id = ? AND {}",
            notification_visible_source_sql("notifications")
        );
        query(&notification_sql)
            .bind(&now)
            .bind(account_id)
            .bind(thread_id)
            .execute(&mut tx)
            .await?;
        let mention_sql = format!(
            "UPDATE mentions SET read_at = COALESCE(read_at, ?)
             WHERE target_account_id = ? AND thread_id = ? AND {}",
            mention_visible_source_sql("mentions")
        );
        query(&mention_sql)
            .bind(&now)
            .bind(account_id)
            .bind(thread_id)
            .execute(&mut tx)
            .await?;
        tx.commit().await?;
        Ok(())
    }

    pub async fn mark_thread_unread(
        &self,
        account_id: &str,
        thread_id: &str,
    ) -> anyhow::Result<()> {
        let mut tx = begin(self.db.write_pool()).await?;
        let thread = load_thread_meta_tx(&mut tx, thread_id).await?;
        ensure_can_view_channel(&mut tx, account_id, &thread.channel_id).await?;
        let last_index: i64 = query_scalar("SELECT last_comment_index FROM threads WHERE id = ?")
            .bind(thread_id)
            .fetch_one(&mut tx)
            .await?;
        let unread_from = last_index.saturating_sub(1);
        let unread_count: i64 = query_scalar(
            "SELECT COUNT(*)
             FROM comments
             WHERE thread_id = ? AND deleted_at IS NULL AND obj_index > ?",
        )
        .bind(thread_id)
        .bind(unread_from)
        .fetch_one(&mut tx)
        .await?;
        query(
            "INSERT INTO thread_reads (thread_id, account_id, last_read_index, unread_count, marked_unread_at)
             VALUES (?, ?, ?, ?, ?)
             ON CONFLICT(thread_id, account_id)
             DO UPDATE SET last_read_index = excluded.last_read_index,
                           unread_count = excluded.unread_count,
                           marked_unread_at = excluded.marked_unread_at",
        )
        .bind(thread_id)
        .bind(account_id)
        .bind(unread_from)
        .bind(unread_count)
        .bind(now())
        .execute(&mut tx)
        .await?;
        tx.commit().await?;
        Ok(())
    }

    pub async fn mark_conversation_read(
        &self,
        account_id: &str,
        conversation_id: &str,
    ) -> anyhow::Result<()> {
        let mut tx = begin(self.db.write_pool()).await?;
        let last_index: Option<i64> = query_scalar(
            "SELECT c.last_message_index
             FROM conversations c
             JOIN conversation_members m
               ON m.conversation_id = c.id AND m.account_id = ?
             WHERE c.id = ?",
        )
        .bind(account_id)
        .bind(conversation_id)
        .fetch_optional(&mut tx)
        .await?;
        let Some(last_index) = last_index else {
            bail!("Not a participant in this conversation");
        };
        query(
            "UPDATE conversation_members
             SET last_read_index = ?, unread_count = 0
             WHERE conversation_id = ? AND account_id = ?",
        )
        .bind(last_index)
        .bind(conversation_id)
        .bind(account_id)
        .execute(&mut tx)
        .await?;
        let notification_sql = format!(
            "UPDATE notifications SET read_at = COALESCE(read_at, ?)
             WHERE account_id = ? AND conversation_id = ? AND {}",
            notification_visible_source_sql("notifications")
        );
        query(&notification_sql)
            .bind(now())
            .bind(account_id)
            .bind(conversation_id)
            .execute(&mut tx)
            .await?;
        let mention_sql = format!(
            "UPDATE mentions SET read_at = COALESCE(read_at, ?)
             WHERE target_account_id = ? AND conversation_id = ? AND {}",
            mention_visible_source_sql("mentions")
        );
        query(&mention_sql)
            .bind(now())
            .bind(account_id)
            .bind(conversation_id)
            .execute(&mut tx)
            .await?;
        tx.commit().await?;
        Ok(())
    }

    pub async fn mark_conversation_unread(
        &self,
        account_id: &str,
        conversation_id: &str,
    ) -> anyhow::Result<()> {
        let mut tx = begin(self.db.write_pool()).await?;
        let last_index: Option<i64> = query_scalar(
            "SELECT c.last_message_index
             FROM conversations c
             JOIN conversation_members m
               ON m.conversation_id = c.id AND m.account_id = ?
             WHERE c.id = ?",
        )
        .bind(account_id)
        .bind(conversation_id)
        .fetch_optional(&mut tx)
        .await?;
        let Some(last_index) = last_index else {
            bail!("Not a participant in this conversation");
        };
        let unread_from = last_index.saturating_sub(1);
        let unread_count: i64 = query_scalar(
            "SELECT COUNT(*)
             FROM conversation_messages
             WHERE conversation_id = ? AND deleted_at IS NULL AND obj_index > ?",
        )
        .bind(conversation_id)
        .bind(unread_from)
        .fetch_one(&mut tx)
        .await?;
        query(
            "UPDATE conversation_members
             SET last_read_index = ?, unread_count = ?
             WHERE conversation_id = ? AND account_id = ?",
        )
        .bind(unread_from)
        .bind(unread_count)
        .bind(conversation_id)
        .bind(account_id)
        .execute(&mut tx)
        .await?;
        tx.commit().await?;
        Ok(())
    }

    pub async fn next_unread(&self, account_id: &str) -> anyhow::Result<Option<NextUnread>> {
        if let Some(row) = query(
            "SELECT t.channel_id, t.id AS thread_id
             FROM threads t
             JOIN channels c ON c.id = t.channel_id
             LEFT JOIN thread_reads r ON r.thread_id = t.id AND r.account_id = ?
             WHERE t.deleted_at IS NULL
               AND t.archived_at IS NULL
               AND (r.muted_until IS NULL OR r.muted_until <= ?)
               AND COALESCE(r.unread_count, t.comment_count) > 0
               AND EXISTS (
                 SELECT 1 FROM channel_members m
                 WHERE m.channel_id = c.id AND m.account_id = ?
               )
             ORDER BY t.last_activity_at DESC
             LIMIT 1",
        )
        .bind(account_id)
        .bind(now())
        .bind(account_id)
        .fetch_optional(self.db.read_pool())
        .await?
        {
            return Ok(Some(NextUnread::Thread {
                channel_id: row.get("channel_id")?,
                thread_id: row.get("thread_id")?,
            }));
        }

        let conversation_id: Option<String> = query_scalar(
            "SELECT c.id
             FROM conversations c
             JOIN conversation_members me ON me.conversation_id = c.id AND me.account_id = ?
             WHERE me.unread_count > 0
               AND c.archived_at IS NULL
               AND (me.muted_until IS NULL OR me.muted_until <= ?)
             ORDER BY c.last_activity_at DESC
             LIMIT 1",
        )
        .bind(account_id)
        .bind(now())
        .fetch_optional(self.db.read_pool())
        .await?;
        Ok(conversation_id.map(|conversation_id| NextUnread::Conversation { conversation_id }))
    }
}
