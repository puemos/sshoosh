use super::*;
impl ServerState {
    pub async fn list_invites(&self, actor_id: &str) -> anyhow::Result<Vec<InviteSummary>> {
        Ok(self
            .list_invites_page(actor_id, PageRequest::first(500))
            .await?
            .items)
    }

    pub async fn list_invites_page(
        &self,
        actor_id: &str,
        request: PageRequest,
    ) -> anyhow::Result<Page<InviteSummary>> {
        let mut tx = begin(self.db.write_pool()).await?;
        require_admin_tx(&mut tx, actor_id).await?;
        let limit = page_limit(request.limit, 500);
        let cursor = decode_cursor(request.cursor.as_deref(), 2)?;
        let cursor_filter = if cursor.is_some() {
            "WHERE i.created_at < ? OR (i.created_at = ? AND i.id < ?)"
        } else {
            ""
        };
        let sql = format!(
            "SELECT i.id, i.role_on_accept, creator.username AS created_by,
                    accepted.username AS accepted_by, i.created_at, i.expires_at,
                    i.revoked_at, i.accepted_at
             FROM invites i
             JOIN accounts creator ON creator.id = i.created_by_account_id
             LEFT JOIN accounts accepted ON accepted.id = i.accepted_by_account_id
             {cursor_filter}
             ORDER BY i.created_at DESC, i.id DESC
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
        let mut items: Vec<InviteSummary> = Vec::new();
        let mut next_cursor = None;
        for (idx, row) in rows.into_iter().enumerate() {
            if idx == limit as usize {
                let last = items.last().expect("last invite row");
                next_cursor = Some(encode_cursor([last.created_at.clone(), last.id.clone()])?);
                break;
            }
            items.push(InviteSummary {
                id: row.get("id"),
                role_on_accept: Role::from_db(row.get::<String>("role_on_accept").as_str())?,
                created_by: row.get("created_by"),
                accepted_by: row.get("accepted_by"),
                created_at: row.get("created_at"),
                expires_at: row.get("expires_at"),
                revoked_at: row.get("revoked_at"),
                accepted_at: row.get("accepted_at"),
            });
        }
        Ok(Page { items, next_cursor })
    }

    pub async fn revoke_invite(&self, actor_id: &str, invite_id: &str) -> anyhow::Result<()> {
        let mut tx = begin(self.db.write_pool()).await?;
        require_admin_tx(&mut tx, actor_id).await?;
        let id: Option<String> = query_scalar(
            "SELECT id FROM invites
             WHERE id LIKE ? AND accepted_at IS NULL AND revoked_at IS NULL",
        )
        .bind(format!("{}%", invite_id.trim()))
        .fetch_optional(&mut tx)
        .await?;
        let Some(id) = id else {
            bail!("Open invite not found");
        };
        let now = now();
        query("UPDATE invites SET revoked_at = ? WHERE id = ?")
            .bind(&now)
            .bind(&id)
            .execute(&mut tx)
            .await?;
        insert_audit(
            &mut tx,
            Some(actor_id),
            "invite.revoked",
            Some(&id),
            serde_json::json!({}),
        )
        .await?;
        insert_event(
            &mut tx,
            None,
            None,
            None,
            "invite.revoked",
            serde_json::json!({"invite_id": id}),
        )
        .await?;
        tx.commit().await?;
        Ok(())
    }

    pub async fn list_channel_members(
        &self,
        actor_id: &str,
        slug: &str,
    ) -> anyhow::Result<Vec<ChannelMemberSummary>> {
        Ok(self
            .list_channel_members_page(actor_id, slug, PageRequest::first(500))
            .await?
            .items)
    }

    pub async fn list_channel_members_page(
        &self,
        actor_id: &str,
        slug: &str,
        request: PageRequest,
    ) -> anyhow::Result<Page<ChannelMemberSummary>> {
        let mut tx = begin(self.db.write_pool()).await?;
        let channel = load_channel_by_slug_tx(&mut tx, slug).await?;
        ensure_can_manage_channel(&mut tx, actor_id, &channel).await?;
        let limit = page_limit(request.limit, 500);
        let cursor = decode_cursor(request.cursor.as_deref(), 1)?;
        let cursor_filter = if cursor.is_some() {
            "AND lower(a.username) > ?"
        } else {
            ""
        };
        let sql = format!(
            "SELECT c.id AS channel_id, c.slug AS channel_slug, a.id AS account_id,
                    a.username, lower(a.username) AS lower_username, m.role, m.joined_at
             FROM channel_members m
             JOIN channels c ON c.id = m.channel_id
             JOIN accounts a ON a.id = m.account_id
             WHERE m.channel_id = ? {cursor_filter}
             ORDER BY lower(a.username), a.id
             LIMIT ?"
        );
        let mut query = query(&sql).bind(&channel.id);
        if let Some(cursor) = cursor {
            query = query.bind(&cursor[0]);
        }
        let rows = query
            .bind(limit.saturating_add(1))
            .fetch_all(&mut tx)
            .await?;
        tx.commit().await?;
        let mut items: Vec<ChannelMemberSummary> = Vec::new();
        let mut next_cursor = None;
        for (idx, row) in rows.into_iter().enumerate() {
            if idx == limit as usize {
                let last = items.last().expect("last channel member row");
                next_cursor = Some(encode_cursor([last.username.to_lowercase()])?);
                break;
            }
            items.push(ChannelMemberSummary {
                channel_id: row.get("channel_id"),
                channel_slug: row.get("channel_slug"),
                username: row.get("username"),
                role: row.get("role"),
                joined_at: row.get("joined_at"),
            });
        }
        Ok(Page { items, next_cursor })
    }

    pub async fn add_channel_member(
        &self,
        actor_id: &str,
        slug: &str,
        username: &str,
    ) -> anyhow::Result<()> {
        update_channel_member(self.db.write_pool(), actor_id, slug, username, true).await
    }

    pub async fn remove_channel_member(
        &self,
        actor_id: &str,
        slug: &str,
        username: &str,
    ) -> anyhow::Result<()> {
        update_channel_member(self.db.write_pool(), actor_id, slug, username, false).await
    }

    pub async fn list_channels(
        &self,
        actor_id: &str,
        include_archived: bool,
    ) -> anyhow::Result<Vec<ChannelDirectoryItem>> {
        Ok(self
            .list_channels_page(actor_id, include_archived, PageRequest::first(500))
            .await?
            .items)
    }

    pub async fn list_channels_page(
        &self,
        actor_id: &str,
        include_archived: bool,
        request: PageRequest,
    ) -> anyhow::Result<Page<ChannelDirectoryItem>> {
        let actor = self.reload_account(actor_id).await?;
        anyhow::ensure!(actor.activated, "Account is not activated");
        let limit = page_limit(request.limit, 500);
        let cursor = decode_cursor(request.cursor.as_deref(), 2)?;
        let cursor_filter = if cursor.is_some() {
            "AND ((CASE WHEN c.slug = 'general' THEN 0 ELSE 1 END) > ?
                OR ((CASE WHEN c.slug = 'general' THEN 0 ELSE 1 END) = ? AND c.slug > ?))"
        } else {
            ""
        };
        let sql = format!(
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
               {cursor_filter}
             ORDER BY CASE WHEN c.slug = 'general' THEN 0 ELSE 1 END, c.slug
             LIMIT ?"
        );
        let mut query = query(&sql)
            .bind(actor_id)
            .bind(include_archived)
            .bind(actor_id)
            .bind(actor.role.as_str());
        if let Some(cursor) = cursor {
            query = query
                .bind(
                    cursor[0]
                        .parse::<i64>()
                        .context("invalid channels cursor")?,
                )
                .bind(
                    cursor[0]
                        .parse::<i64>()
                        .context("invalid channels cursor")?,
                )
                .bind(&cursor[1]);
        }
        let rows = query
            .bind(limit.saturating_add(1))
            .fetch_all(self.db.read_pool())
            .await?;
        let mut items: Vec<ChannelDirectoryItem> = Vec::new();
        let mut next_cursor = None;
        for (idx, row) in rows.into_iter().enumerate() {
            if idx == limit as usize {
                let last = items.last().expect("last channel row");
                let group = if last.slug == "general" { "0" } else { "1" };
                next_cursor = Some(encode_cursor([group.to_string(), last.slug.clone()])?);
                break;
            }
            items.push(ChannelDirectoryItem {
                id: row.get("id"),
                slug: row.get("slug"),
                name: row.get("name"),
                visibility: row.get("visibility"),
                topic: row
                    .get::<Option<String>>("topic")
                    .map(|topic| sanitize_single_line_text(&topic)),
                joined: row.get::<i64>("joined") != 0,
                archived: row.get::<Option<String>>("archived_at").is_some(),
            });
        }
        Ok(Page { items, next_cursor })
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
        query("DELETE FROM channel_members WHERE channel_id = ? AND account_id = ?")
            .bind(&channel.id)
            .bind(actor_id)
            .execute(&mut tx)
            .await?;
        insert_audit(
            &mut tx,
            Some(actor_id),
            "channel.left",
            Some(&channel.id),
            serde_json::json!({"channel": channel.slug}),
        )
        .await?;
        insert_event(
            &mut tx,
            Some(&channel.id),
            None,
            None,
            "channel.left",
            serde_json::json!({"channel_id": channel.id, "account_id": actor_id}),
        )
        .await?;
        tx.commit().await?;
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
        query("UPDATE channels SET slug = ?, name = ?, updated_at = ? WHERE id = ?")
            .bind(&next_slug)
            .bind(&next_slug)
            .bind(&now)
            .bind(&channel.id)
            .execute(&mut tx)
            .await?;
        insert_audit(
            &mut tx,
            Some(actor_id),
            "channel.renamed",
            Some(&channel.id),
            serde_json::json!({"from": channel.slug, "to": next_slug}),
        )
        .await?;
        insert_event(
            &mut tx,
            Some(&channel.id),
            None,
            None,
            "channel.renamed",
            serde_json::json!({"channel_id": channel.id, "slug": next_slug}),
        )
        .await?;
        tx.commit().await?;
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
        let topic = sanitize_single_line_text(topic);
        let topic = topic.trim();
        let topic = (!topic.is_empty()).then_some(topic);
        let now = now();
        query("UPDATE channels SET topic = ?, updated_at = ? WHERE id = ?")
            .bind(topic)
            .bind(&now)
            .bind(&channel.id)
            .execute(&mut tx)
            .await?;
        insert_audit(
            &mut tx,
            Some(actor_id),
            "channel.topic_changed",
            Some(&channel.id),
            serde_json::json!({"channel": channel.slug, "topic": topic}),
        )
        .await?;
        insert_event(
            &mut tx,
            Some(&channel.id),
            None,
            None,
            "channel.topic_changed",
            serde_json::json!({"channel_id": channel.id}),
        )
        .await?;
        tx.commit().await?;
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
        query("UPDATE channels SET archived_at = ?, archived_by_account_id = ?, updated_at = ? WHERE id = ?")
            .bind(archived.then_some(now.as_str()))
            .bind(archived.then_some(actor_id))
            .bind(&now)
            .bind(&channel.id)
            .execute(&mut tx)
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
        insert_event(
            &mut tx,
            Some(&channel.id),
            None,
            None,
            action,
            serde_json::json!({"channel_id": channel.id}),
        )
        .await?;
        tx.commit().await?;
        Ok(())
    }
}
