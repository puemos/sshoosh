impl ServerState {
    pub async fn new(db: Database) -> anyhow::Result<Self> {
        let (live_tx, _) = broadcast::channel(1024);
        let (tx, rx) = mpsc::channel(256);
        let max_seq: i64 = sqlx::query_scalar("SELECT COALESCE(MAX(seq), 0) FROM event_log")
            .fetch_one(db.read_pool())
            .await
            .unwrap_or(0);
        let event_cursor = Arc::new(RwLock::new(max_seq));
        let state = Self {
            db: db.clone(),
            writer: WriteHandle { tx },
            live_tx,
            active_connections: Arc::new(RwLock::new(HashMap::new())),
        };
        start_writer(db.write_pool().clone(), state.live_tx.clone(), rx);
        start_event_poller(
            db.read_pool().clone(),
            state.live_tx.clone(),
            event_cursor.clone(),
        );
        start_webhook_worker(db.write_pool().clone());
        Ok(state)
    }

    pub fn subscribe(&self) -> broadcast::Receiver<LiveEvent> {
        self.live_tx.subscribe()
    }

    pub async fn ensure_account_for_key(
        &self,
        login_username: &str,
        fingerprint: &str,
        public_key: &str,
    ) -> anyhow::Result<Account> {
        let mut tx = self.db.write_pool().begin().await?;
        let now = now();

        if let Some(row) = sqlx::query(
            "SELECT a.id, a.username, a.display_name, a.role, a.activated_at
             FROM ssh_keys k
             JOIN accounts a ON a.id = k.account_id
             WHERE k.fingerprint = ? AND k.revoked_at IS NULL AND a.disabled_at IS NULL",
        )
        .bind(fingerprint)
        .fetch_optional(&mut *tx)
        .await?
        {
            let account_id: String = row.get("id");
            sqlx::query("UPDATE accounts SET last_seen_at = ?, updated_at = ? WHERE id = ?")
                .bind(&now)
                .bind(&now)
                .bind(&account_id)
                .execute(&mut *tx)
                .await?;
            sqlx::query("UPDATE ssh_keys SET last_used_at = ? WHERE fingerprint = ?")
                .bind(&now)
                .bind(fingerprint)
                .execute(&mut *tx)
                .await?;
            tx.commit().await?;
            return Ok(account_from_row(row));
        }

        let existing_count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM accounts")
            .fetch_one(&mut *tx)
            .await?;
        let account_id = id();
        let username = next_username(&mut tx, login_username).await?;
        let role = if existing_count == 0 {
            Role::Owner
        } else {
            Role::Member
        };
        let activated_at = if existing_count == 0 {
            Some(now.clone())
        } else {
            None
        };

        sqlx::query(
            "INSERT INTO accounts
             (id, username, display_name, role, settings_json, created_at, updated_at, last_seen_at, activated_at)
             VALUES (?, ?, ?, ?, '{}', ?, ?, ?, ?)",
        )
        .bind(&account_id)
        .bind(&username)
        .bind(&username)
        .bind(role.as_str())
        .bind(&now)
        .bind(&now)
        .bind(&now)
        .bind(&activated_at)
        .execute(&mut *tx)
        .await?;
        sqlx::query(
            "INSERT INTO ssh_keys (id, account_id, fingerprint, public_key, label, created_at, last_used_at)
             VALUES (?, ?, ?, ?, 'default', ?, ?)",
        )
        .bind(id())
        .bind(&account_id)
        .bind(fingerprint)
        .bind(public_key)
        .bind(&now)
        .bind(&now)
        .execute(&mut *tx)
        .await?;

        if existing_count == 0 {
            let channel_id = id();
            sqlx::query(
                "INSERT INTO channels
                 (id, slug, name, visibility, topic, created_by_account_id, created_at, updated_at)
                 VALUES (?, 'general', 'general', 'public', 'General discussion', ?, ?, ?)",
            )
            .bind(&channel_id)
            .bind(&account_id)
            .bind(&now)
            .bind(&now)
            .execute(&mut *tx)
            .await?;
            sqlx::query(
                "INSERT INTO channel_members (channel_id, account_id, role, joined_at)
                 VALUES (?, ?, 'owner', ?)",
            )
            .bind(&channel_id)
            .bind(&account_id)
            .bind(&now)
            .execute(&mut *tx)
            .await?;
            insert_event(
                &mut tx,
                None,
                None,
                None,
                "channel.created",
                serde_json::json!({"channel_id": channel_id, "slug": "general"}),
            )
            .await?;
        }

        tx.commit().await?;
        Ok(Account {
            id: account_id,
            username: username.clone(),
            display_name: username,
            role,
            activated: activated_at.is_some(),
        })
    }

    pub async fn reload_account(&self, account_id: &str) -> anyhow::Result<Account> {
        let row = sqlx::query(
            "SELECT id, username, display_name, role, activated_at
             FROM accounts WHERE id = ? AND disabled_at IS NULL",
        )
        .bind(account_id)
        .fetch_one(self.db.read_pool())
        .await?;
        Ok(account_from_row(row))
    }

    pub async fn snapshot(
        &self,
        account_id: &str,
        selected_channel_id: Option<&str>,
        selected_thread_id: Option<&str>,
        selected_conversation_id: Option<&str>,
    ) -> anyhow::Result<Snapshot> {
        self.snapshot_with_history_limit(
            account_id,
            selected_channel_id,
            selected_thread_id,
            selected_conversation_id,
            DEFAULT_HISTORY_LIMIT,
        )
        .await
    }

    pub async fn snapshot_with_history_limit(
        &self,
        account_id: &str,
        selected_channel_id: Option<&str>,
        selected_thread_id: Option<&str>,
        selected_conversation_id: Option<&str>,
        history_limit: i64,
    ) -> anyhow::Result<Snapshot> {
        let history_limit = history_limit.clamp(1, MAX_HISTORY_LIMIT);
        let account = self.reload_account(account_id).await?;
        if !account.activated {
            return Ok(Snapshot::default());
        }

        let channels = load_channels(self.db.read_pool(), account_id).await?;
        let mut active_account_ids = load_active_presence_sessions(self.db.read_pool()).await?;
        active_account_ids.extend(self.active_account_ids().await);
        let users = load_user_presence(self.db.read_pool(), &active_account_ids).await?;
        let selected_channel_id = selected_channel_id
            .filter(|id| channels.iter().any(|channel| channel.id == *id))
            .map(ToOwned::to_owned)
            .or_else(|| channels.first().map(|channel| channel.id.clone()));

        let threads = if let Some(channel_id) = selected_channel_id.as_deref() {
            load_threads(self.db.read_pool(), account_id, channel_id).await?
        } else {
            Vec::new()
        };
        let selected_thread_id = selected_thread_id
            .filter(|id| threads.iter().any(|thread| thread.id == *id))
            .map(ToOwned::to_owned)
            .or_else(|| threads.first().map(|thread| thread.id.clone()));
        let (comments, comments_has_more) = if let Some(thread_id) = selected_thread_id.as_deref() {
            load_comments(self.db.read_pool(), thread_id, history_limit).await?
        } else {
            (Vec::new(), false)
        };

        let conversations = load_conversations(self.db.read_pool(), account_id).await?;
        let selected_conversation_id = selected_conversation_id
            .filter(|id| {
                conversations
                    .iter()
                    .any(|conversation| conversation.id == *id)
            })
            .map(ToOwned::to_owned);
        let (conversation_messages, conversation_messages_has_more) = if let Some(conversation_id) =
            selected_conversation_id.as_deref()
        {
            load_conversation_messages(self.db.read_pool(), conversation_id, history_limit).await?
        } else {
            (Vec::new(), false)
        };
        let notifications = load_notifications(self.db.read_pool(), account_id, 20).await?;
        let notification_unread_count: i64 = sqlx::query_scalar(
            "SELECT COUNT(*) FROM notifications WHERE account_id = ? AND read_at IS NULL",
        )
        .bind(account_id)
        .fetch_one(self.db.read_pool())
        .await?;
        let mention_unread_count: i64 = sqlx::query_scalar(
            "SELECT COUNT(*) FROM mentions WHERE target_account_id = ? AND read_at IS NULL",
        )
        .bind(account_id)
        .fetch_one(self.db.read_pool())
        .await?;

        Ok(Snapshot {
            current_username: Some(account.username),
            users,
            channels,
            threads,
            comments,
            conversations,
            conversation_messages,
            comments_has_more,
            conversation_messages_has_more,
            search_query: None,
            search_results: Vec::new(),
            search_has_more: false,
            notifications,
            notification_unread_count,
            mention_unread_count,
            selected_channel_id,
            selected_thread_id,
            selected_conversation_id,
        })
    }

    pub async fn touch_account(&self, account_id: &str) -> anyhow::Result<()> {
        self.record_presence(account_id, None, false).await
    }

    pub async fn touch_account_session(
        &self,
        account_id: &str,
        session_id: &str,
    ) -> anyhow::Result<()> {
        self.record_presence(account_id, Some(session_id), false)
            .await
    }

    async fn record_presence(
        &self,
        account_id: &str,
        session_id: Option<&str>,
        disconnected: bool,
    ) -> anyhow::Result<()> {
        let mut tx = begin(self.db.write_pool()).await?;
        let username: Option<String> = sqlx::query_scalar(
            "SELECT username FROM accounts
             WHERE id = ? AND activated_at IS NOT NULL AND disabled_at IS NULL",
        )
        .bind(account_id)
        .fetch_optional(&mut *tx)
        .await?;
        let Some(username) = username else {
            tx.commit().await?;
            return Ok(());
        };
        let now = now();
        sqlx::query("UPDATE accounts SET last_seen_at = ?, updated_at = ? WHERE id = ?")
            .bind(&now)
            .bind(&now)
            .bind(account_id)
            .execute(&mut *tx)
            .await?;
        if let Some(session_id) = session_id {
            if disconnected {
                sqlx::query(
                    "UPDATE presence_sessions
                     SET last_seen_at = ?, disconnected_at = COALESCE(disconnected_at, ?)
                     WHERE id = ? AND account_id = ?",
                )
                .bind(&now)
                .bind(&now)
                .bind(session_id)
                .bind(account_id)
                .execute(&mut *tx)
                .await?;
            } else {
                sqlx::query(
                    "INSERT INTO presence_sessions (id, account_id, started_at, last_seen_at)
                     VALUES (?, ?, ?, ?)
                     ON CONFLICT(id) DO UPDATE SET last_seen_at = excluded.last_seen_at
                     WHERE presence_sessions.disconnected_at IS NULL",
                )
                .bind(session_id)
                .bind(account_id)
                .bind(&now)
                .bind(&now)
                .execute(&mut *tx)
                .await?;
            }
        }
        let event = insert_event(
            &mut tx,
            None,
            None,
            None,
            "presence.updated",
            serde_json::json!({"account_id": account_id, "username": username}),
        )
        .await?;
        tx.commit().await?;
        publish(&self.live_tx, event);
        Ok(())
    }

    pub async fn begin_account_session(&self, account_id: &str) -> anyhow::Result<String> {
        let session_id = id();
        {
            let mut active_connections = self.active_connections.write().await;
            *active_connections
                .entry(account_id.to_string())
                .or_default() += 1;
        }
        if let Err(err) = self.touch_account_session(account_id, &session_id).await {
            self.remove_account_session(account_id).await;
            return Err(err);
        }
        Ok(session_id)
    }

    pub async fn end_account_session(&self, account_id: &str) -> anyhow::Result<()> {
        let session_id = self.latest_open_presence_session(account_id).await?;
        self.end_account_session_inner(account_id, session_id.as_deref())
            .await
    }

    pub async fn end_presence_session(
        &self,
        account_id: &str,
        session_id: &str,
    ) -> anyhow::Result<()> {
        self.end_account_session_inner(account_id, Some(session_id))
            .await
    }

    async fn end_account_session_inner(
        &self,
        account_id: &str,
        session_id: Option<&str>,
    ) -> anyhow::Result<()> {
        let disconnected = self.remove_account_session(account_id).await;
        if let Some(session_id) = session_id {
            self.record_presence(account_id, Some(session_id), true)
                .await?;
        } else if disconnected {
            self.touch_account(account_id).await?;
        }
        Ok(())
    }

    async fn latest_open_presence_session(
        &self,
        account_id: &str,
    ) -> anyhow::Result<Option<String>> {
        sqlx::query_scalar(
            "SELECT id
             FROM presence_sessions
             WHERE account_id = ? AND disconnected_at IS NULL
             ORDER BY last_seen_at DESC, started_at DESC
             LIMIT 1",
        )
        .bind(account_id)
        .fetch_optional(self.db.read_pool())
        .await
        .map_err(Into::into)
    }

    async fn remove_account_session(&self, account_id: &str) -> bool {
        let mut active_connections = self.active_connections.write().await;
        let Some(count) = active_connections.get_mut(account_id) else {
            return false;
        };
        if *count > 1 {
            *count -= 1;
            false
        } else {
            active_connections.remove(account_id);
            true
        }
    }

    async fn active_account_ids(&self) -> HashSet<String> {
        self.active_connections
            .read()
            .await
            .keys()
            .cloned()
            .collect()
    }

    pub async fn create_invite(&self, actor_id: String) -> anyhow::Result<String> {
        self.writer.create_invite(actor_id).await
    }

    pub async fn accept_invite(
        &self,
        account_id: String,
        code: String,
        username: String,
    ) -> anyhow::Result<()> {
        self.writer.accept_invite(account_id, code, username).await
    }

    pub async fn create_channel(
        &self,
        actor_id: String,
        name: String,
        private: bool,
    ) -> anyhow::Result<String> {
        self.writer.create_channel(actor_id, name, private).await
    }

    pub async fn join_channel(&self, actor_id: String, slug: String) -> anyhow::Result<String> {
        self.writer.join_channel(actor_id, slug).await
    }

    pub async fn create_thread(
        &self,
        actor_id: String,
        channel_id: String,
        title: String,
        body: String,
    ) -> anyhow::Result<String> {
        self.writer
            .create_thread(actor_id, channel_id, title, body)
            .await
    }

    pub async fn add_comment(
        &self,
        actor_id: String,
        thread_id: String,
        body: String,
    ) -> anyhow::Result<()> {
        self.writer.add_comment(actor_id, thread_id, body).await
    }

    pub async fn open_dm(&self, actor_id: String, target: String) -> anyhow::Result<String> {
        self.writer.open_dm(actor_id, target).await
    }

    pub async fn send_dm(
        &self,
        actor_id: String,
        conversation_id: String,
        body: String,
    ) -> anyhow::Result<()> {
        self.writer.send_dm(actor_id, conversation_id, body).await
    }


}
