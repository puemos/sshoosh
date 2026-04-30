impl ServerState {
    pub async fn edit_thread(
        &self,
        actor_id: &str,
        thread_id: &str,
        title: &str,
        body: &str,
    ) -> anyhow::Result<()> {
        let mut tx = begin(self.db.write_pool()).await?;
        let thread = load_thread_meta_tx(&mut tx, thread_id).await?;
        ensure_can_modify_thread(&mut tx, actor_id, &thread, false).await?;
        let title = title.trim();
        let body = body.trim();
        anyhow::ensure!(!title.is_empty(), "Thread title is required");
        anyhow::ensure!(!body.is_empty(), "Thread body is required");
        let next_key = normalize_name_key(title);
        if next_key != normalize_name_key(&thread.title) {
            ensure_thread_name_available(&mut tx, &thread.channel_id, &next_key).await?;
        }
        let now = now();
        sqlx::query(
            "UPDATE threads SET title = ?, body = ?, updated_at = ?, edited_at = ? WHERE id = ?",
        )
        .bind(title)
        .bind(body)
        .bind(&now)
        .bind(&now)
        .bind(thread_id)
        .execute(&mut *tx)
        .await?;
        let channel_slug: String = sqlx::query_scalar("SELECT slug FROM channels WHERE id = ?")
            .bind(&thread.channel_id)
            .fetch_one(&mut *tx)
            .await?;
        upsert_search_index_tx(
            &mut tx,
            SearchIndexInput {
                kind: "thread",
                object_id: thread_id,
                channel_id: Some(&thread.channel_id),
                thread_id: Some(thread_id),
                conversation_id: None,
                title,
                body,
                context: &format!("#{channel_slug}"),
            },
        )
        .await?;
        insert_audit(
            &mut tx,
            Some(actor_id),
            "thread.edited",
            Some(thread_id),
            serde_json::json!({"channel_id": thread.channel_id}),
        )
        .await?;
        let event = insert_event(
            &mut tx,
            Some(&thread.channel_id),
            Some(thread_id),
            None,
            "thread.edited",
            serde_json::json!({"thread_id": thread_id}),
        )
        .await?;
        tx.commit().await?;
        publish(&self.live_tx, event);
        Ok(())
    }

    pub async fn rename_thread(
        &self,
        actor_id: &str,
        thread_id: &str,
        title: &str,
    ) -> anyhow::Result<()> {
        let mut tx = begin(self.db.write_pool()).await?;
        let thread = load_thread_meta_tx(&mut tx, thread_id).await?;
        ensure_can_modify_thread(&mut tx, actor_id, &thread, false).await?;
        let title = title.trim();
        anyhow::ensure!(!title.is_empty(), "Thread title is required");
        let next_key = normalize_name_key(title);
        if next_key != normalize_name_key(&thread.title) {
            ensure_thread_name_available(&mut tx, &thread.channel_id, &next_key).await?;
        }
        let now = now();
        sqlx::query("UPDATE threads SET title = ?, updated_at = ?, edited_at = ? WHERE id = ?")
            .bind(title)
            .bind(&now)
            .bind(&now)
            .bind(thread_id)
            .execute(&mut *tx)
            .await?;
        let channel_slug: String = sqlx::query_scalar("SELECT slug FROM channels WHERE id = ?")
            .bind(&thread.channel_id)
            .fetch_one(&mut *tx)
            .await?;
        upsert_search_index_tx(
            &mut tx,
            SearchIndexInput {
                kind: "thread",
                object_id: thread_id,
                channel_id: Some(&thread.channel_id),
                thread_id: Some(thread_id),
                conversation_id: None,
                title,
                body: &thread.body,
                context: &format!("#{channel_slug}"),
            },
        )
        .await?;
        insert_audit(
            &mut tx,
            Some(actor_id),
            "thread.edited",
            Some(thread_id),
            serde_json::json!({"channel_id": thread.channel_id}),
        )
        .await?;
        let event = insert_event(
            &mut tx,
            Some(&thread.channel_id),
            Some(thread_id),
            None,
            "thread.edited",
            serde_json::json!({"thread_id": thread_id}),
        )
        .await?;
        tx.commit().await?;
        publish(&self.live_tx, event);
        Ok(())
    }

    pub async fn delete_thread(&self, actor_id: &str, thread_id: &str) -> anyhow::Result<()> {
        update_thread_flag(
            self.db.write_pool(),
            &self.live_tx,
            actor_id,
            thread_id,
            ThreadFlag::Deleted,
            true,
        )
        .await
    }

    pub async fn set_thread_archived(
        &self,
        actor_id: &str,
        thread_id: &str,
        archived: bool,
    ) -> anyhow::Result<()> {
        update_thread_flag(
            self.db.write_pool(),
            &self.live_tx,
            actor_id,
            thread_id,
            ThreadFlag::Archived,
            archived,
        )
        .await
    }

    pub async fn set_thread_pinned(
        &self,
        actor_id: &str,
        thread_id: &str,
        pinned: bool,
    ) -> anyhow::Result<()> {
        update_thread_flag(
            self.db.write_pool(),
            &self.live_tx,
            actor_id,
            thread_id,
            ThreadFlag::Pinned,
            pinned,
        )
        .await
    }

    pub async fn set_thread_muted(
        &self,
        actor_id: &str,
        thread_id: &str,
        ttl_hours: Option<i64>,
    ) -> anyhow::Result<()> {
        let mut tx = begin(self.db.write_pool()).await?;
        let thread = load_thread_meta_tx(&mut tx, thread_id).await?;
        ensure_can_view_channel(&mut tx, actor_id, &thread.channel_id).await?;
        let muted_until = ttl_hours.and_then(timestamp_after_hours);
        upsert_thread_read_state(
            &mut tx,
            actor_id,
            thread_id,
            true,
            muted_until.as_deref(),
            false,
            None,
        )
        .await?;
        insert_audit(
            &mut tx,
            Some(actor_id),
            if muted_until.is_some() {
                "thread.muted"
            } else {
                "thread.unmuted"
            },
            Some(thread_id),
            serde_json::json!({}),
        )
        .await?;
        tx.commit().await?;
        Ok(())
    }

    pub async fn set_thread_saved(
        &self,
        actor_id: &str,
        thread_id: &str,
        saved: bool,
    ) -> anyhow::Result<()> {
        let mut tx = begin(self.db.write_pool()).await?;
        let thread = load_thread_meta_tx(&mut tx, thread_id).await?;
        ensure_can_view_channel(&mut tx, actor_id, &thread.channel_id).await?;
        let saved_at = saved.then(now);
        upsert_thread_read_state(
            &mut tx,
            actor_id,
            thread_id,
            false,
            None,
            true,
            saved_at.as_deref(),
        )
        .await?;
        insert_audit(
            &mut tx,
            Some(actor_id),
            if saved {
                "thread.saved"
            } else {
                "thread.unsaved"
            },
            Some(thread_id),
            serde_json::json!({}),
        )
        .await?;
        tx.commit().await?;
        Ok(())
    }

    pub async fn edit_comment(
        &self,
        actor_id: &str,
        thread_id: &str,
        obj_index: i64,
        body: &str,
    ) -> anyhow::Result<()> {
        update_comment_body(
            self.db.write_pool(),
            &self.live_tx,
            actor_id,
            thread_id,
            obj_index,
            body,
        )
        .await
    }

    pub async fn delete_comment(
        &self,
        actor_id: &str,
        thread_id: &str,
        obj_index: i64,
    ) -> anyhow::Result<()> {
        soft_delete_comment(
            self.db.write_pool(),
            &self.live_tx,
            actor_id,
            thread_id,
            obj_index,
        )
        .await
    }

    pub async fn edit_dm(
        &self,
        actor_id: &str,
        conversation_id: &str,
        obj_index: i64,
        body: &str,
    ) -> anyhow::Result<()> {
        update_dm_body(
            self.db.write_pool(),
            &self.live_tx,
            actor_id,
            conversation_id,
            obj_index,
            body,
        )
        .await
    }

    pub async fn delete_dm(
        &self,
        actor_id: &str,
        conversation_id: &str,
        obj_index: i64,
    ) -> anyhow::Result<()> {
        soft_delete_dm(
            self.db.write_pool(),
            &self.live_tx,
            actor_id,
            conversation_id,
            obj_index,
        )
        .await
    }

    pub async fn set_conversation_muted(
        &self,
        actor_id: &str,
        conversation_id: &str,
        ttl_hours: Option<i64>,
    ) -> anyhow::Result<()> {
        let muted_until = ttl_hours.and_then(timestamp_after_hours);
        sqlx::query(
            "UPDATE conversation_members SET muted_until = ? WHERE conversation_id = ? AND account_id = ?",
        )
        .bind(muted_until.as_deref())
        .bind(conversation_id)
        .bind(actor_id)
        .execute(self.db.write_pool())
        .await?;
        Ok(())
    }

    pub async fn set_conversation_saved(
        &self,
        actor_id: &str,
        conversation_id: &str,
        saved: bool,
    ) -> anyhow::Result<()> {
        let saved_at = saved.then(now);
        sqlx::query(
            "UPDATE conversation_members SET saved_at = ? WHERE conversation_id = ? AND account_id = ?",
        )
        .bind(saved_at.as_deref())
        .bind(conversation_id)
        .bind(actor_id)
        .execute(self.db.write_pool())
        .await?;
        Ok(())
    }

    pub async fn search(
        &self,
        actor_id: &str,
        query: &str,
        limit: i64,
    ) -> anyhow::Result<Vec<SearchResult>> {
        Ok(self.search_page(actor_id, query, limit).await?.results)
    }

    pub async fn search_page(
        &self,
        actor_id: &str,
        query: &str,
        limit: i64,
    ) -> anyhow::Result<SearchPage> {
        search_visible(self.db.read_pool(), actor_id, query, limit).await
    }


}
