impl ServerState {
    pub async fn list_webhooks(
        &self,
        actor_id: &str,
    ) -> anyhow::Result<(Vec<WebhookSummary>, Vec<WebhookDeliverySummary>)> {
        let mut tx = begin(self.db.write_pool()).await?;
        require_admin_tx(&mut tx, actor_id).await?;
        let webhook_rows = sqlx::query(
            "SELECT id, name, url, enabled, created_at, updated_at, disabled_at
             FROM webhook_subscriptions
             ORDER BY created_at DESC",
        )
        .fetch_all(&mut *tx)
        .await?;
        let delivery_rows = sqlx::query(
            "SELECT j.id, w.name AS webhook_name, j.status, j.attempts, j.next_attempt_at,
                    j.last_error, j.created_at, j.delivered_at
             FROM webhook_jobs j
             JOIN webhook_subscriptions w ON w.id = j.webhook_id
             ORDER BY j.created_at DESC
             LIMIT 50",
        )
        .fetch_all(&mut *tx)
        .await?;
        tx.commit().await?;
        Ok((
            webhook_rows
                .into_iter()
                .map(|row| WebhookSummary {
                    id: row.get("id"),
                    name: row.get("name"),
                    url: row.get("url"),
                    enabled: row.get::<i64, _>("enabled") != 0,
                    created_at: row.get("created_at"),
                    updated_at: row.get("updated_at"),
                    disabled_at: row.get("disabled_at"),
                })
                .collect(),
            delivery_rows
                .into_iter()
                .map(|row| WebhookDeliverySummary {
                    id: row.get("id"),
                    webhook_name: row.get("webhook_name"),
                    status: row.get("status"),
                    attempts: row.get("attempts"),
                    next_attempt_at: row.get("next_attempt_at"),
                    last_error: row.get("last_error"),
                    created_at: row.get("created_at"),
                    delivered_at: row.get("delivered_at"),
                })
                .collect(),
        ))
    }

    pub async fn add_webhook(
        &self,
        actor_id: &str,
        name: &str,
        url: &str,
    ) -> anyhow::Result<String> {
        anyhow::ensure!(
            url.starts_with("http://") || url.starts_with("https://"),
            "Webhook URL must be http(s)"
        );
        let mut tx = begin(self.db.write_pool()).await?;
        require_admin_tx(&mut tx, actor_id).await?;
        let now = now();
        let id = id();
        sqlx::query(
            "INSERT INTO webhook_subscriptions
             (id, created_by_account_id, name, url, enabled, created_at, updated_at)
             VALUES (?, ?, ?, ?, 1, ?, ?)",
        )
        .bind(&id)
        .bind(actor_id)
        .bind(name.trim())
        .bind(url.trim())
        .bind(&now)
        .bind(&now)
        .execute(&mut *tx)
        .await?;
        insert_audit(
            &mut tx,
            Some(actor_id),
            "webhook.added",
            Some(&id),
            serde_json::json!({"name": name.trim(), "url": url.trim()}),
        )
        .await?;
        tx.commit().await?;
        Ok(id)
    }

    pub async fn remove_webhook(&self, actor_id: &str, webhook: &str) -> anyhow::Result<()> {
        let mut tx = begin(self.db.write_pool()).await?;
        require_admin_tx(&mut tx, actor_id).await?;
        let row = sqlx::query(
            "SELECT id, name FROM webhook_subscriptions WHERE id LIKE ? AND disabled_at IS NULL",
        )
        .bind(format!("{}%", webhook.trim()))
        .fetch_optional(&mut *tx)
        .await?;
        let Some(row) = row else {
            bail!("Active webhook not found");
        };
        let id: String = row.get("id");
        let now = now();
        sqlx::query("UPDATE webhook_subscriptions SET enabled = 0, disabled_at = ?, updated_at = ? WHERE id = ?")
            .bind(&now)
            .bind(&now)
            .bind(&id)
            .execute(&mut *tx)
            .await?;
        insert_audit(
            &mut tx,
            Some(actor_id),
            "webhook.removed",
            Some(&id),
            serde_json::json!({"name": row.get::<String, _>("name")}),
        )
        .await?;
        tx.commit().await?;
        Ok(())
    }

    pub async fn test_webhook(&self, actor_id: &str, webhook: &str) -> anyhow::Result<()> {
        let mut tx = begin(self.db.write_pool()).await?;
        require_admin_tx(&mut tx, actor_id).await?;
        let row = sqlx::query("SELECT id, name FROM webhook_subscriptions WHERE id LIKE ? AND enabled = 1 AND disabled_at IS NULL")
            .bind(format!("{}%", webhook.trim()))
            .fetch_optional(&mut *tx)
            .await?;
        let Some(row) = row else {
            bail!("Active webhook not found");
        };
        let webhook_id: String = row.get("id");
        let webhook_name: String = row.get("name");
        let now = now();
        let payload = serde_json::json!({
            "kind": "webhook_test",
            "title": "sshoosh webhook test",
            "body": "Webhook delivery test",
        });
        sqlx::query(
            "INSERT INTO webhook_jobs
             (id, webhook_id, payload_json, status, attempts, next_attempt_at, created_at, updated_at)
             VALUES (?, ?, ?, 'pending', 0, ?, ?, ?)",
        )
        .bind(id())
        .bind(&webhook_id)
        .bind(serde_json::to_string(&payload)?)
        .bind(&now)
        .bind(&now)
        .bind(&now)
        .execute(&mut *tx)
        .await?;
        insert_audit(
            &mut tx,
            Some(actor_id),
            "webhook.test_queued",
            Some(&webhook_id),
            serde_json::json!({"name": webhook_name}),
        )
        .await?;
        tx.commit().await?;
        Ok(())
    }

    pub async fn list_audit(&self, actor_id: &str, limit: i64) -> anyhow::Result<Vec<AuditEntry>> {
        let mut tx = begin(self.db.write_pool()).await?;
        require_admin_tx(&mut tx, actor_id).await?;
        let rows = sqlx::query(
            "SELECT l.id, actor.username AS actor_username, l.action, l.target,
                    l.metadata_json, l.created_at
             FROM audit_log l
             LEFT JOIN accounts actor ON actor.id = l.actor_account_id
             ORDER BY l.created_at DESC
             LIMIT ?",
        )
        .bind(limit.clamp(1, 500))
        .fetch_all(&mut *tx)
        .await?;
        tx.commit().await?;
        Ok(rows
            .into_iter()
            .map(|row| AuditEntry {
                id: row.get("id"),
                actor_username: row.get("actor_username"),
                action: row.get("action"),
                target: row.get("target"),
                metadata_json: row.get("metadata_json"),
                created_at: row.get("created_at"),
            })
            .collect())
    }

    pub async fn export_workspace(
        &self,
        actor_id: &str,
        format: ExportFormat,
        include_audit: bool,
    ) -> anyhow::Result<String> {
        let mut tx = begin(self.db.write_pool()).await?;
        require_admin_tx(&mut tx, actor_id).await?;
        let accounts = rows_to_json(
            sqlx::query("SELECT id, username, display_name, role, created_at, activated_at, disabled_at FROM accounts ORDER BY username")
                .fetch_all(&mut *tx)
                .await?,
        )?;
        let channels = rows_to_json(
            sqlx::query("SELECT id, slug, name, visibility, topic, created_at, archived_at FROM channels ORDER BY slug")
                .fetch_all(&mut *tx)
                .await?,
        )?;
        let threads = rows_to_json(
            sqlx::query("SELECT id, channel_id, creator_account_id, title, body, comment_count, created_at, edited_at, archived_at, deleted_at FROM threads ORDER BY created_at")
                .fetch_all(&mut *tx)
                .await?,
        )?;
        let comments = rows_to_json(
            sqlx::query("SELECT id, thread_id, channel_id, author_account_id, obj_index, body, created_at, edited_at, deleted_at FROM comments ORDER BY created_at")
                .fetch_all(&mut *tx)
                .await?,
        )?;
        let dms = rows_to_json(
            sqlx::query("SELECT id, dm_key, creator_account_id, created_at, archived_at FROM conversations ORDER BY created_at")
                .fetch_all(&mut *tx)
                .await?,
        )?;
        let dm_messages = rows_to_json(
            sqlx::query("SELECT id, conversation_id, author_account_id, obj_index, body, created_at, edited_at, deleted_at FROM conversation_messages ORDER BY created_at")
                .fetch_all(&mut *tx)
                .await?,
        )?;
        let mentions = rows_to_json(
            sqlx::query("SELECT * FROM mentions ORDER BY created_at")
                .fetch_all(&mut *tx)
                .await?,
        )?;
        let reactions = rows_to_json(
            sqlx::query("SELECT * FROM reactions ORDER BY created_at")
                .fetch_all(&mut *tx)
                .await?,
        )?;
        let notifications = rows_to_json(
            sqlx::query("SELECT * FROM notifications ORDER BY created_at")
                .fetch_all(&mut *tx)
                .await?,
        )?;
        let webhooks = rows_to_json(
            sqlx::query("SELECT id, name, url, enabled, created_at, updated_at, disabled_at FROM webhook_subscriptions ORDER BY created_at")
                .fetch_all(&mut *tx)
                .await?,
        )?;
        let audit = if include_audit {
            rows_to_json(
                sqlx::query("SELECT * FROM audit_log ORDER BY created_at")
                    .fetch_all(&mut *tx)
                    .await?,
            )?
        } else {
            serde_json::Value::Array(Vec::new())
        };
        tx.commit().await?;
        let bundle = serde_json::json!({
            "exported_at": now(),
            "users": accounts,
            "channels": channels,
            "threads": threads,
            "comments": comments,
            "dms": dms,
            "dm_messages": dm_messages,
            "mentions": mentions,
            "reactions": reactions,
            "notifications": notifications,
            "webhooks": webhooks,
            "audit": audit,
        });
        match format {
            ExportFormat::Json => Ok(serde_json::to_string_pretty(&bundle)?),
            ExportFormat::Markdown => Ok(export_markdown(&bundle)),
        }
    }

    pub async fn mark_thread_read(&self, account_id: &str, thread_id: &str) -> anyhow::Result<()> {
        let mut tx = begin(self.db.write_pool()).await?;
        let last_index: i64 =
            sqlx::query_scalar("SELECT last_comment_index FROM threads WHERE id = ?")
                .bind(thread_id)
                .fetch_one(&mut *tx)
                .await?;
        sqlx::query(
            "INSERT INTO thread_reads (thread_id, account_id, last_read_index, marked_unread_at)
             VALUES (?, ?, ?, NULL)
             ON CONFLICT(thread_id, account_id)
             DO UPDATE SET last_read_index = excluded.last_read_index, marked_unread_at = NULL",
        )
        .bind(thread_id)
        .bind(account_id)
        .bind(last_index)
        .execute(&mut *tx)
        .await?;
        let now = now();
        sqlx::query(
            "UPDATE notifications SET read_at = COALESCE(read_at, ?)
             WHERE account_id = ? AND thread_id = ?",
        )
        .bind(&now)
        .bind(account_id)
        .bind(thread_id)
        .execute(&mut *tx)
        .await?;
        sqlx::query(
            "UPDATE mentions SET read_at = COALESCE(read_at, ?)
             WHERE target_account_id = ? AND thread_id = ?",
        )
        .bind(&now)
        .bind(account_id)
        .bind(thread_id)
        .execute(&mut *tx)
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
        let last_index: i64 =
            sqlx::query_scalar("SELECT last_comment_index FROM threads WHERE id = ?")
                .bind(thread_id)
                .fetch_one(&mut *tx)
                .await?;
        let unread_from = last_index.saturating_sub(1);
        sqlx::query(
            "INSERT INTO thread_reads (thread_id, account_id, last_read_index, marked_unread_at)
             VALUES (?, ?, ?, ?)
             ON CONFLICT(thread_id, account_id)
             DO UPDATE SET last_read_index = excluded.last_read_index, marked_unread_at = excluded.marked_unread_at",
        )
        .bind(thread_id)
        .bind(account_id)
        .bind(unread_from)
        .bind(now())
        .execute(&mut *tx)
        .await?;
        tx.commit().await?;
        Ok(())
    }

    pub async fn mark_conversation_read(
        &self,
        account_id: &str,
        conversation_id: &str,
    ) -> anyhow::Result<()> {
        let last_index: i64 =
            sqlx::query_scalar("SELECT last_message_index FROM conversations WHERE id = ?")
                .bind(conversation_id)
                .fetch_one(self.db.read_pool())
                .await?;
        sqlx::query(
            "UPDATE conversation_members SET last_read_index = ? WHERE conversation_id = ? AND account_id = ?",
        )
        .bind(last_index)
        .bind(conversation_id)
        .bind(account_id)
        .execute(self.db.write_pool())
        .await?;
        sqlx::query(
            "UPDATE notifications SET read_at = COALESCE(read_at, ?)
             WHERE account_id = ? AND conversation_id = ?",
        )
        .bind(now())
        .bind(account_id)
        .bind(conversation_id)
        .execute(self.db.write_pool())
        .await?;
        sqlx::query(
            "UPDATE mentions SET read_at = COALESCE(read_at, ?)
             WHERE target_account_id = ? AND conversation_id = ?",
        )
        .bind(now())
        .bind(account_id)
        .bind(conversation_id)
        .execute(self.db.write_pool())
        .await?;
        Ok(())
    }

    pub async fn mark_conversation_unread(
        &self,
        account_id: &str,
        conversation_id: &str,
    ) -> anyhow::Result<()> {
        let last_index: i64 =
            sqlx::query_scalar("SELECT last_message_index FROM conversations WHERE id = ?")
                .bind(conversation_id)
                .fetch_one(self.db.read_pool())
                .await?;
        sqlx::query(
            "UPDATE conversation_members SET last_read_index = ? WHERE conversation_id = ? AND account_id = ?",
        )
        .bind(last_index.saturating_sub(1))
        .bind(conversation_id)
        .bind(account_id)
        .execute(self.db.write_pool())
        .await?;
        Ok(())
    }

    pub async fn next_unread(&self, account_id: &str) -> anyhow::Result<Option<NextUnread>> {
        if let Some(row) = sqlx::query(
            "SELECT t.channel_id, t.id AS thread_id
             FROM threads t
             JOIN channels c ON c.id = t.channel_id
             LEFT JOIN thread_reads r ON r.thread_id = t.id AND r.account_id = ?
             WHERE t.deleted_at IS NULL
               AND t.archived_at IS NULL
               AND (r.muted_until IS NULL OR r.muted_until <= ?)
               AND (
                 SELECT COUNT(*)
                 FROM comments cm
                 WHERE cm.thread_id = t.id
                   AND cm.deleted_at IS NULL
                   AND cm.obj_index > COALESCE(r.last_read_index, 0)
               ) > 0
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
                channel_id: row.get("channel_id"),
                thread_id: row.get("thread_id"),
            }));
        }

        let conversation_id: Option<String> = sqlx::query_scalar(
            "SELECT c.id
             FROM conversations c
             JOIN conversation_members me ON me.conversation_id = c.id AND me.account_id = ?
             WHERE (
                 SELECT COUNT(*)
                 FROM conversation_messages msg
                 WHERE msg.conversation_id = c.id
                   AND msg.deleted_at IS NULL
                   AND msg.obj_index > me.last_read_index
               ) > 0
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
