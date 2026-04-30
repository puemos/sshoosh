impl ServerState {
    pub async fn list_notifications(
        &self,
        account_id: &str,
        limit: i64,
    ) -> anyhow::Result<Vec<NotificationSummary>> {
        load_notifications(self.db.read_pool(), account_id, limit).await
    }

    pub async fn mark_notification_read(
        &self,
        account_id: &str,
        notification_id: Option<&str>,
    ) -> anyhow::Result<()> {
        let now = now();
        if let Some(notification_id) = notification_id {
            sqlx::query(
                "UPDATE notifications SET read_at = ?
                 WHERE account_id = ? AND (id = ? OR id LIKE ?)",
            )
            .bind(&now)
            .bind(account_id)
            .bind(notification_id)
            .bind(format!("{}%", notification_id.trim()))
            .execute(self.db.write_pool())
            .await?;
        } else {
            sqlx::query(
                "UPDATE notifications SET read_at = ? WHERE account_id = ? AND read_at IS NULL",
            )
            .bind(&now)
            .bind(account_id)
            .execute(self.db.write_pool())
            .await?;
        }
        Ok(())
    }

    pub async fn list_mentions(
        &self,
        account_id: &str,
        limit: i64,
    ) -> anyhow::Result<Vec<MentionSummary>> {
        let limit = limit.clamp(1, 200);
        let rows = sqlx::query(
            "SELECT m.id, actor.username AS actor_username, m.source_kind,
                    COALESCE(t.title, 'DM') AS title,
                    COALESCE(cm.body, dm.body, t.body, '') AS body,
                    m.created_at, m.read_at
             FROM mentions m
             JOIN accounts actor ON actor.id = m.actor_account_id
             LEFT JOIN threads t ON t.id = m.thread_id
             LEFT JOIN comments cm ON cm.id = m.source_id AND m.source_kind = 'comment'
             LEFT JOIN conversation_messages dm ON dm.id = m.source_id AND m.source_kind = 'dm'
             WHERE m.target_account_id = ?
             ORDER BY m.created_at DESC
             LIMIT ?",
        )
        .bind(account_id)
        .bind(limit)
        .fetch_all(self.db.read_pool())
        .await?;
        Ok(rows
            .into_iter()
            .map(|row| MentionSummary {
                id: row.get("id"),
                actor_username: row.get("actor_username"),
                source_kind: row.get("source_kind"),
                title: row.get("title"),
                body: row.get("body"),
                created_at: row.get("created_at"),
                read_at: row.get("read_at"),
            })
            .collect())
    }

    pub async fn react_to_thread(
        &self,
        account_id: &str,
        thread_id: &str,
        emoji: &str,
        remove: bool,
    ) -> anyhow::Result<()> {
        let mut tx = begin(self.db.write_pool()).await?;
        let thread = load_thread_meta_tx(&mut tx, thread_id).await?;
        ensure_can_view_channel(&mut tx, account_id, &thread.channel_id).await?;
        set_reaction_tx(&mut tx, account_id, "thread", thread_id, emoji, remove).await?;
        let event = insert_event(
            &mut tx,
            Some(&thread.channel_id),
            Some(thread_id),
            None,
            if remove {
                "reaction.removed"
            } else {
                "reaction.added"
            },
            serde_json::json!({"source_kind": "thread", "source_id": thread_id}),
        )
        .await?;
        tx.commit().await?;
        publish(&self.live_tx, event);
        Ok(())
    }

    pub async fn react_to_comment(
        &self,
        account_id: &str,
        thread_id: &str,
        obj_index: i64,
        emoji: &str,
        remove: bool,
    ) -> anyhow::Result<()> {
        let mut tx = begin(self.db.write_pool()).await?;
        let thread = load_thread_meta_tx(&mut tx, thread_id).await?;
        ensure_can_view_channel(&mut tx, account_id, &thread.channel_id).await?;
        let comment = load_comment_meta_tx(&mut tx, thread_id, obj_index).await?;
        set_reaction_tx(&mut tx, account_id, "comment", &comment.id, emoji, remove).await?;
        let event = insert_event(
            &mut tx,
            Some(&thread.channel_id),
            Some(thread_id),
            None,
            if remove {
                "reaction.removed"
            } else {
                "reaction.added"
            },
            serde_json::json!({"source_kind": "comment", "source_id": comment.id}),
        )
        .await?;
        tx.commit().await?;
        publish(&self.live_tx, event);
        Ok(())
    }

    pub async fn react_to_dm(
        &self,
        account_id: &str,
        conversation_id: &str,
        obj_index: i64,
        emoji: &str,
        remove: bool,
    ) -> anyhow::Result<()> {
        let mut tx = begin(self.db.write_pool()).await?;
        let message =
            load_dm_message_meta_tx(&mut tx, account_id, conversation_id, obj_index).await?;
        set_reaction_tx(&mut tx, account_id, "dm", &message.id, emoji, remove).await?;
        let event = insert_event(
            &mut tx,
            None,
            None,
            Some(conversation_id),
            if remove {
                "reaction.removed"
            } else {
                "reaction.added"
            },
            serde_json::json!({"source_kind": "dm", "source_id": message.id}),
        )
        .await?;
        tx.commit().await?;
        publish(&self.live_tx, event);
        Ok(())
    }


}
