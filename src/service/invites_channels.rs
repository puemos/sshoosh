impl ServerState {
    pub async fn list_invites(&self, actor_id: &str) -> anyhow::Result<Vec<InviteSummary>> {
        let mut tx = begin(self.db.write_pool()).await?;
        require_admin_tx(&mut tx, actor_id).await?;
        let rows = sqlx::query(
            "SELECT i.id, i.role_on_accept, creator.username AS created_by,
                    accepted.username AS accepted_by, i.created_at, i.expires_at,
                    i.revoked_at, i.accepted_at
             FROM invites i
             JOIN accounts creator ON creator.id = i.created_by_account_id
             LEFT JOIN accounts accepted ON accepted.id = i.accepted_by_account_id
             ORDER BY i.created_at DESC",
        )
        .fetch_all(&mut *tx)
        .await?;
        tx.commit().await?;
        Ok(rows
            .into_iter()
            .map(|row| InviteSummary {
                id: row.get("id"),
                role_on_accept: Role::from_db(row.get::<String, _>("role_on_accept").as_str()),
                created_by: row.get("created_by"),
                accepted_by: row.get("accepted_by"),
                created_at: row.get("created_at"),
                expires_at: row.get("expires_at"),
                revoked_at: row.get("revoked_at"),
                accepted_at: row.get("accepted_at"),
            })
            .collect())
    }

    pub async fn revoke_invite(&self, actor_id: &str, invite_id: &str) -> anyhow::Result<()> {
        let mut tx = begin(self.db.write_pool()).await?;
        require_admin_tx(&mut tx, actor_id).await?;
        let id: Option<String> = sqlx::query_scalar(
            "SELECT id FROM invites
             WHERE id LIKE ? AND accepted_at IS NULL AND revoked_at IS NULL",
        )
        .bind(format!("{}%", invite_id.trim()))
        .fetch_optional(&mut *tx)
        .await?;
        let Some(id) = id else {
            bail!("Open invite not found");
        };
        let now = now();
        sqlx::query("UPDATE invites SET revoked_at = ? WHERE id = ?")
            .bind(&now)
            .bind(&id)
            .execute(&mut *tx)
            .await?;
        insert_audit(
            &mut tx,
            Some(actor_id),
            "invite.revoked",
            Some(&id),
            serde_json::json!({}),
        )
        .await?;
        let event = insert_event(
            &mut tx,
            None,
            None,
            None,
            "invite.revoked",
            serde_json::json!({"invite_id": id}),
        )
        .await?;
        tx.commit().await?;
        publish(&self.live_tx, event);
        Ok(())
    }

    pub async fn list_channel_members(
        &self,
        actor_id: &str,
        slug: &str,
    ) -> anyhow::Result<Vec<ChannelMemberSummary>> {
        let mut tx = begin(self.db.write_pool()).await?;
        let channel = load_channel_by_slug_tx(&mut tx, slug).await?;
        ensure_can_manage_channel(&mut tx, actor_id, &channel).await?;
        let rows = sqlx::query(
            "SELECT c.id AS channel_id, c.slug AS channel_slug, a.username, m.role, m.joined_at
             FROM channel_members m
             JOIN channels c ON c.id = m.channel_id
             JOIN accounts a ON a.id = m.account_id
             WHERE m.channel_id = ?
             ORDER BY a.username",
        )
        .bind(&channel.id)
        .fetch_all(&mut *tx)
        .await?;
        tx.commit().await?;
        Ok(rows
            .into_iter()
            .map(|row| ChannelMemberSummary {
                channel_id: row.get("channel_id"),
                channel_slug: row.get("channel_slug"),
                username: row.get("username"),
                role: row.get("role"),
                joined_at: row.get("joined_at"),
            })
            .collect())
    }

    pub async fn add_channel_member(
        &self,
        actor_id: &str,
        slug: &str,
        username: &str,
    ) -> anyhow::Result<()> {
        update_channel_member(
            self.db.write_pool(),
            &self.live_tx,
            actor_id,
            slug,
            username,
            true,
        )
        .await
    }

    pub async fn remove_channel_member(
        &self,
        actor_id: &str,
        slug: &str,
        username: &str,
    ) -> anyhow::Result<()> {
        update_channel_member(
            self.db.write_pool(),
            &self.live_tx,
            actor_id,
            slug,
            username,
            false,
        )
        .await
    }

    pub async fn list_channels(
        &self,
        actor_id: &str,
        include_archived: bool,
    ) -> anyhow::Result<Vec<ChannelDirectoryItem>> {
        let actor = self.reload_account(actor_id).await?;
        anyhow::ensure!(actor.activated, "Account is not activated");
        let rows = sqlx::query(
            "SELECT c.id, c.slug, c.name, c.visibility, c.topic, c.archived_at,
                    EXISTS (
                      SELECT 1 FROM channel_members m
                      WHERE m.channel_id = c.id AND m.account_id = ?
                    ) AS joined
             FROM channels c
             WHERE (? OR c.archived_at IS NULL)
               AND (
                 c.visibility = 'public'
                 OR EXISTS (
                   SELECT 1 FROM channel_members m
                   WHERE m.channel_id = c.id AND m.account_id = ?
                 )
                 OR ? IN ('owner', 'admin')
               )
             ORDER BY CASE WHEN c.slug = 'general' THEN 0 ELSE 1 END, c.slug",
        )
        .bind(actor_id)
        .bind(include_archived)
        .bind(actor_id)
        .bind(actor.role.as_str())
        .fetch_all(self.db.read_pool())
        .await?;
        Ok(rows
            .into_iter()
            .map(|row| ChannelDirectoryItem {
                id: row.get("id"),
                slug: row.get("slug"),
                name: row.get("name"),
                visibility: row.get("visibility"),
                topic: row.get("topic"),
                joined: row.get::<i64, _>("joined") != 0,
                archived: row.get::<Option<String>, _>("archived_at").is_some(),
            })
            .collect())
    }

    pub async fn leave_channel(&self, actor_id: &str, slug: &str) -> anyhow::Result<()> {
        let mut tx = begin(self.db.write_pool()).await?;
        let actor = load_account_tx(&mut tx, actor_id).await?;
        anyhow::ensure!(actor.activated, "Account is not activated");
        let channel = load_channel_by_slug_tx(&mut tx, slug).await?;
        anyhow::ensure!(channel.slug != "general", "#general cannot be left");
        anyhow::ensure!(
            channel.created_by_account_id != actor_id,
            "Channel creator cannot leave without archiving or transferring ownership"
        );
        sqlx::query("DELETE FROM channel_members WHERE channel_id = ? AND account_id = ?")
            .bind(&channel.id)
            .bind(actor_id)
            .execute(&mut *tx)
            .await?;
        insert_audit(
            &mut tx,
            Some(actor_id),
            "channel.left",
            Some(&channel.id),
            serde_json::json!({"channel": channel.slug}),
        )
        .await?;
        let event = insert_event(
            &mut tx,
            Some(&channel.id),
            None,
            None,
            "channel.left",
            serde_json::json!({"channel_id": channel.id, "account_id": actor_id}),
        )
        .await?;
        tx.commit().await?;
        publish(&self.live_tx, event);
        Ok(())
    }

    pub async fn rename_channel(
        &self,
        actor_id: &str,
        slug: &str,
        next_name: &str,
    ) -> anyhow::Result<()> {
        let next_slug = normalize_slug(next_name)?;
        let mut tx = begin(self.db.write_pool()).await?;
        let channel = load_channel_by_slug_tx(&mut tx, slug).await?;
        ensure_can_manage_channel(&mut tx, actor_id, &channel).await?;
        anyhow::ensure!(channel.slug != "general", "#general cannot be renamed");
        if channel.slug != next_slug {
            ensure_channel_name_available(&mut tx, &next_slug).await?;
        }
        let now = now();
        sqlx::query("UPDATE channels SET slug = ?, name = ?, updated_at = ? WHERE id = ?")
            .bind(&next_slug)
            .bind(&next_slug)
            .bind(&now)
            .bind(&channel.id)
            .execute(&mut *tx)
            .await?;
        insert_audit(
            &mut tx,
            Some(actor_id),
            "channel.renamed",
            Some(&channel.id),
            serde_json::json!({"from": channel.slug, "to": next_slug}),
        )
        .await?;
        let event = insert_event(
            &mut tx,
            Some(&channel.id),
            None,
            None,
            "channel.renamed",
            serde_json::json!({"channel_id": channel.id, "slug": next_slug}),
        )
        .await?;
        tx.commit().await?;
        publish(&self.live_tx, event);
        Ok(())
    }

    pub async fn set_channel_topic(
        &self,
        actor_id: &str,
        slug: &str,
        topic: &str,
    ) -> anyhow::Result<()> {
        let mut tx = begin(self.db.write_pool()).await?;
        let channel = load_channel_by_slug_tx(&mut tx, slug).await?;
        ensure_can_manage_channel(&mut tx, actor_id, &channel).await?;
        let topic = topic.trim();
        let topic = (!topic.is_empty()).then_some(topic);
        let now = now();
        sqlx::query("UPDATE channels SET topic = ?, updated_at = ? WHERE id = ?")
            .bind(topic)
            .bind(&now)
            .bind(&channel.id)
            .execute(&mut *tx)
            .await?;
        insert_audit(
            &mut tx,
            Some(actor_id),
            "channel.topic_changed",
            Some(&channel.id),
            serde_json::json!({"channel": channel.slug, "topic": topic}),
        )
        .await?;
        let event = insert_event(
            &mut tx,
            Some(&channel.id),
            None,
            None,
            "channel.topic_changed",
            serde_json::json!({"channel_id": channel.id}),
        )
        .await?;
        tx.commit().await?;
        publish(&self.live_tx, event);
        Ok(())
    }

    pub async fn set_channel_archived(
        &self,
        actor_id: &str,
        slug: &str,
        archived: bool,
    ) -> anyhow::Result<()> {
        let mut tx = begin(self.db.write_pool()).await?;
        let channel = if archived {
            load_channel_by_slug_tx(&mut tx, slug).await?
        } else {
            load_channel_by_slug_any_tx(&mut tx, slug).await?
        };
        ensure_can_manage_channel(&mut tx, actor_id, &channel).await?;
        anyhow::ensure!(channel.slug != "general", "#general cannot be archived");
        let now = now();
        sqlx::query("UPDATE channels SET archived_at = ?, archived_by_account_id = ?, updated_at = ? WHERE id = ?")
            .bind(archived.then_some(now.as_str()))
            .bind(archived.then_some(actor_id))
            .bind(&now)
            .bind(&channel.id)
            .execute(&mut *tx)
            .await?;
        let action = if archived {
            "channel.archived"
        } else {
            "channel.unarchived"
        };
        insert_audit(
            &mut tx,
            Some(actor_id),
            action,
            Some(&channel.id),
            serde_json::json!({"channel": channel.slug}),
        )
        .await?;
        let event = insert_event(
            &mut tx,
            Some(&channel.id),
            None,
            None,
            action,
            serde_json::json!({"channel_id": channel.id}),
        )
        .await?;
        tx.commit().await?;
        publish(&self.live_tx, event);
        Ok(())
    }


}
