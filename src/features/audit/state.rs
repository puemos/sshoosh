use super::*;
impl ServerState {
    pub async fn list_audit(&self, actor_id: &str, limit: i64) -> anyhow::Result<Vec<AuditEntry>> {
        Ok(self
            .list_audit_page(actor_id, PageRequest::first(limit))
            .await?
            .items)
    }

    pub async fn list_audit_page(
        &self,
        actor_id: &str,
        request: PageRequest,
    ) -> anyhow::Result<Page<AuditEntry>> {
        let mut tx = begin(self.db.write_pool()).await?;
        require_admin_tx(&mut tx, actor_id).await?;
        let limit = page_limit(request.limit, 500);
        let cursor = decode_cursor(request.cursor.as_deref(), 2)?;
        let cursor_filter = if cursor.is_some() {
            "WHERE l.created_at < ? OR (l.created_at = ? AND l.id < ?)"
        } else {
            ""
        };
        let sql = format!(
            "SELECT l.id, actor.username AS actor_username, l.action, l.target,
                    l.metadata_json, l.created_at
             FROM audit_log l
             LEFT JOIN accounts actor ON actor.id = l.actor_account_id
             {cursor_filter}
             ORDER BY l.created_at DESC, l.id DESC
             LIMIT ?"
        );
        let mut query = query(&sql);
        if let Some(cursor) = cursor {
            query = query.bind(&cursor[0]).bind(&cursor[0]).bind(&cursor[1]);
        }
        let rows = query
            .bind(limit.saturating_add(1))
            .fetch_all(&mut tx)
            .await?;
        tx.commit().await?;
        let mut items: Vec<AuditEntry> = Vec::new();
        let mut next_cursor = None;
        for (idx, row) in rows.into_iter().enumerate() {
            if idx == limit as usize {
                let last = items.last().expect("last audit row");
                next_cursor = Some(encode_cursor([last.created_at.clone(), last.id.clone()])?);
                break;
            }
            items.push(AuditEntry {
                id: row.get("id")?,
                actor_username: row.get("actor_username")?,
                action: row.get("action")?,
                target: row.get("target")?,
                metadata_json: row.get("metadata_json")?,
                created_at: row.get("created_at")?,
            });
        }
        Ok(Page { items, next_cursor })
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
            query("SELECT id, username, display_name, role, created_at, activated_at, disabled_at FROM accounts ORDER BY username")
                .fetch_all(&mut tx)
                .await?,
        )?;
        let channels = rows_to_json(
            query("SELECT id, slug, name, visibility, topic, created_at, archived_at FROM channels ORDER BY slug")
                .fetch_all(&mut tx)
                .await?,
        )?;
        let threads = rows_to_json(
            query("SELECT id, channel_id, creator_account_id, title, body, comment_count, created_at, edited_at, archived_at, deleted_at FROM threads ORDER BY created_at")
                .fetch_all(&mut tx)
                .await?,
        )?;
        let comments = rows_to_json(
            query("SELECT id, thread_id, channel_id, author_account_id, obj_index, body, created_at, edited_at, deleted_at FROM comments ORDER BY created_at")
                .fetch_all(&mut tx)
                .await?,
        )?;
        let dms = rows_to_json(
            query("SELECT id, dm_key, creator_account_id, created_at, archived_at FROM conversations ORDER BY created_at")
                .fetch_all(&mut tx)
                .await?,
        )?;
        let dm_messages = rows_to_json(
            query("SELECT id, conversation_id, author_account_id, obj_index, body, created_at, edited_at, deleted_at FROM conversation_messages ORDER BY created_at")
                .fetch_all(&mut tx)
                .await?,
        )?;
        let mentions = rows_to_json(
            query("SELECT * FROM mentions ORDER BY created_at")
                .fetch_all(&mut tx)
                .await?,
        )?;
        let reactions = rows_to_json(
            query("SELECT * FROM reactions ORDER BY created_at")
                .fetch_all(&mut tx)
                .await?,
        )?;
        let notifications = rows_to_json(
            query("SELECT * FROM notifications ORDER BY created_at")
                .fetch_all(&mut tx)
                .await?,
        )?;
        let audit = if include_audit {
            rows_to_json(
                query("SELECT * FROM audit_log ORDER BY created_at")
                    .fetch_all(&mut tx)
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
            "audit": audit,
        });
        match format {
            ExportFormat::Json => Ok(serde_json::to_string_pretty(&bundle)?),
            ExportFormat::Markdown => Ok(export_markdown(&bundle)),
        }
    }
}
