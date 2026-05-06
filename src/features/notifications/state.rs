use super::*;
impl ServerState {
    pub async fn terminal_notifications_enabled(&self, account_id: &str) -> anyhow::Result<bool> {
        let settings = account_settings(self.db.read_pool(), account_id).await?;
        Ok(settings
            .get("terminal_notifications")
            .and_then(serde_json::Value::as_bool)
            .unwrap_or(false))
    }

    pub async fn set_terminal_notifications(
        &self,
        account_id: &str,
        enabled: bool,
    ) -> anyhow::Result<()> {
        let mut settings = account_settings(self.db.read_pool(), account_id).await?;
        let object = settings
            .as_object_mut()
            .ok_or_else(|| anyhow::anyhow!("account settings must be a JSON object"))?;
        object.insert(
            "terminal_notifications".to_string(),
            serde_json::Value::Bool(enabled),
        );
        query("UPDATE accounts SET settings_json = ?, updated_at = ? WHERE id = ?")
            .bind(serde_json::to_string(&settings)?)
            .bind(now())
            .bind(account_id)
            .execute(self.db.write_pool())
            .await?;
        Ok(())
    }

    pub async fn list_notifications(
        &self,
        account_id: &str,
        limit: i64,
    ) -> anyhow::Result<Vec<NotificationSummary>> {
        Ok(self
            .list_notifications_page(account_id, PageRequest::first(limit))
            .await?
            .items)
    }

    pub async fn list_notifications_page(
        &self,
        account_id: &str,
        request: PageRequest,
    ) -> anyhow::Result<Page<NotificationSummary>> {
        load_notifications_page(self.db.read_pool(), account_id, request).await
    }

    pub async fn mark_notification_read(
        &self,
        account_id: &str,
        notification_id: Option<&str>,
    ) -> anyhow::Result<()> {
        let now = now();
        if let Some(notification_id) = notification_id {
            let sql = format!(
                "UPDATE notifications SET read_at = ?
                 WHERE account_id = ?
                   AND (id = ? OR id LIKE ?)
                   AND archived_at IS NULL
                   AND {}",
                notification_visible_source_sql("notifications")
            );
            query(&sql)
                .bind(&now)
                .bind(account_id)
                .bind(notification_id)
                .bind(format!("{}%", notification_id.trim()))
                .execute(self.db.write_pool())
                .await?;
        } else {
            let sql = format!(
                "UPDATE notifications SET read_at = ?
                 WHERE account_id = ? AND read_at IS NULL AND archived_at IS NULL AND {}",
                notification_visible_source_sql("notifications")
            );
            query(&sql)
                .bind(&now)
                .bind(account_id)
                .execute(self.db.write_pool())
                .await?;
        }
        Ok(())
    }

    pub async fn archive_notifications(&self, account_id: &str) -> anyhow::Result<()> {
        let now = now();
        let sql = format!(
            "UPDATE notifications SET archived_at = ?
             WHERE account_id = ? AND archived_at IS NULL AND {}",
            notification_visible_source_sql("notifications")
        );
        query(&sql)
            .bind(&now)
            .bind(account_id)
            .execute(self.db.write_pool())
            .await?;
        Ok(())
    }

    pub async fn list_mentions(
        &self,
        account_id: &str,
        limit: i64,
    ) -> anyhow::Result<Vec<MentionSummary>> {
        let limit = limit.clamp(1, 200);
        let sql = format!(
            "SELECT m.id, actor.username AS actor_username, m.source_kind, m.source_id,
                    COALESCE(m.obj_index, cm.obj_index, dm.obj_index) AS source_obj_index,
                    m.channel_id, c.slug AS channel_slug,
                    m.thread_id, t.title AS thread_title,
                    m.conversation_id,
                    COALESCE(t.title, 'DM') AS title,
                    COALESCE(cm.body, dm.body, t.body, '') AS body,
                    m.created_at, m.read_at
             FROM mentions m
             JOIN accounts actor ON actor.id = m.actor_account_id
             LEFT JOIN channels c ON c.id = m.channel_id
             LEFT JOIN threads t ON t.id = m.thread_id
             LEFT JOIN comments cm ON cm.id = m.source_id AND m.source_kind = 'comment'
             LEFT JOIN conversation_messages dm ON dm.id = m.source_id AND m.source_kind = 'dm'
             WHERE m.target_account_id = ?
               AND {}
             ORDER BY m.created_at DESC
             LIMIT ?",
            mention_visible_source_sql("m")
        );
        let rows = query(&sql)
            .bind(account_id)
            .bind(limit)
            .fetch_all(self.db.read_pool())
            .await?;
        rows.into_iter()
            .map(|row| {
                Ok(MentionSummary {
                    id: row.get("id")?,
                    actor_username: row.get("actor_username")?,
                    source_kind: row.get("source_kind")?,
                    source_id: row.get("source_id")?,
                    source_obj_index: row.get("source_obj_index")?,
                    channel_id: row.get("channel_id")?,
                    channel_slug: row.get("channel_slug")?,
                    thread_id: row.get("thread_id")?,
                    thread_title: row
                        .get::<Option<String>>("thread_title")?
                        .map(|title| sanitize_single_line_text(&title)),
                    conversation_id: row.get("conversation_id")?,
                    title: sanitize_single_line_text(&row.get::<String>("title")?),
                    body: sanitize_stored_text(&row.get::<String>("body")?),
                    created_at: row.get("created_at")?,
                    read_at: row.get("read_at")?,
                })
            })
            .collect()
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
        insert_event(
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
        insert_event(
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
        insert_event(
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
        Ok(())
    }
}

async fn account_settings(pool: &Database, account_id: &str) -> anyhow::Result<serde_json::Value> {
    let settings_json: String =
        query_scalar("SELECT settings_json FROM accounts WHERE id = ? AND disabled_at IS NULL")
            .bind(account_id)
            .fetch_one(pool)
            .await?;
    let settings = serde_json::from_str(&settings_json)
        .unwrap_or_else(|_| serde_json::Value::Object(serde_json::Map::new()));
    Ok(if settings.is_object() {
        settings
    } else {
        serde_json::Value::Object(serde_json::Map::new())
    })
}

#[cfg(test)]
mod terminal_notification_settings_tests {
    use super::*;

    #[tokio::test]
    async fn terminal_notifications_default_false_and_preserve_other_settings() {
        let db_path = std::env::temp_dir().join(format!(
            "sshoosh-notification-settings-{}.sqlite",
            Uuid::now_v7()
        ));
        let db = Database::connect(&db_path).await.expect("connect db");
        db.init().await.expect("init db");
        let state = ServerState::new(db).await.expect("state");
        let token = state
            .create_bootstrap_token()
            .await
            .expect("bootstrap token");
        let pending = state
            .redeem_token_for_key(
                "owner",
                &token,
                "SHA256:terminal-settings",
                "ssh-ed25519 terminal-settings",
            )
            .await
            .expect("account");
        let account = state
            .complete_onboarding(&pending.id, "owner")
            .await
            .expect("complete account");

        assert!(
            !state
                .terminal_notifications_enabled(&account.id)
                .await
                .expect("default setting")
        );

        query("UPDATE accounts SET settings_json = ? WHERE id = ?")
            .bind(r#"{"theme":"dark"}"#)
            .bind(&account.id)
            .execute(state.db.write_pool())
            .await
            .expect("seed setting");

        state
            .set_terminal_notifications(&account.id, true)
            .await
            .expect("enable setting");

        assert!(
            state
                .terminal_notifications_enabled(&account.id)
                .await
                .expect("enabled setting")
        );
        let settings_json: String = query_scalar("SELECT settings_json FROM accounts WHERE id = ?")
            .bind(&account.id)
            .fetch_one(state.db.read_pool())
            .await
            .expect("settings json");
        let settings: serde_json::Value =
            serde_json::from_str(&settings_json).expect("valid settings");
        assert_eq!(settings["theme"], "dark");
        assert_eq!(settings["terminal_notifications"], true);
    }
}
