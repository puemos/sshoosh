use super::*;

impl ServerState {
    pub async fn new(db: Database) -> anyhow::Result<Self> {
        let (live_tx, _) = broadcast::channel(1024);
        Ok(Self {
            db,
            live_tx,
            active_connections: Arc::new(RwLock::new(HashMap::new())),
        })
    }

    pub fn subscribe(&self) -> broadcast::Receiver<LiveEvent> {
        self.live_tx.subscribe()
    }

    pub fn is_master(&self) -> bool {
        self.db.is_master()
    }

    /// Look up an account by SSH key fingerprint. Returns `Ok(None)` when the
    /// fingerprint is not registered. Pending accounts are allowed so a user who
    /// already redeemed a token can reconnect and finish choosing a username.
    pub async fn lookup_active_account_for_key(
        &self,
        fingerprint: &str,
    ) -> anyhow::Result<Option<Account>> {
        let mut tx = self.db.write_pool().begin().await?;
        let now = now();
        let Some(row) = query(
            "SELECT a.id, a.username, a.display_name, a.role, a.activated_at, a.pending_username
             FROM ssh_keys k
             JOIN accounts a ON a.id = k.account_id
             WHERE k.fingerprint = ?
               AND k.revoked_at IS NULL
               AND a.disabled_at IS NULL",
        )
        .bind(fingerprint)
        .fetch_optional(&mut tx)
        .await?
        else {
            tx.commit().await?;
            return Ok(None);
        };
        let account_id: String = row.get("id")?;
        let activated = row.get::<Option<String>>("activated_at")?.is_some();
        if activated {
            query("UPDATE accounts SET last_seen_at = ?, updated_at = ? WHERE id = ?")
                .bind(&now)
                .bind(&now)
                .bind(&account_id)
                .execute(&mut tx)
                .await?;
        }
        query("UPDATE ssh_keys SET last_used_at = ? WHERE fingerprint = ?")
            .bind(&now)
            .bind(fingerprint)
            .execute(&mut tx)
            .await?;
        tx.commit().await?;
        Ok(Some(account_from_row(row)?))
    }

    /// Redeem a device-link, bootstrap, or invite token for an SSH login.
    /// Device-link tokens bind the supplied key to an existing account; bootstrap
    /// and invite tokens create a pending account that finishes setup in the TUI.
    pub async fn redeem_ssh_login_token_for_key(
        &self,
        desired_username: &str,
        token: &str,
        fingerprint: &str,
        public_key: &str,
    ) -> anyhow::Result<Account> {
        if let Some(account) = self
            .redeem_device_link_token_for_key(token, fingerprint, public_key)
            .await?
        {
            return Ok(account);
        }
        self.redeem_token_for_key(desired_username, token, fingerprint, public_key)
            .await
    }

    async fn redeem_device_link_token_for_key(
        &self,
        token: &str,
        fingerprint: &str,
        public_key: &str,
    ) -> anyhow::Result<Option<Account>> {
        let mut tx = self.db.write_pool().begin().await?;
        let now = now();
        let token_hash = code_hash(token);
        let Some(row) = query(
            "SELECT t.id, t.account_id, t.label, t.expires_at, t.used_at,
                    a.username, a.display_name, a.role, a.activated_at, a.disabled_at
             FROM device_link_tokens t
             JOIN accounts a ON a.id = t.account_id
             WHERE t.code_hash = ?
             LIMIT 1",
        )
        .bind(&token_hash)
        .fetch_optional(&mut tx)
        .await?
        else {
            tx.commit().await?;
            return Ok(None);
        };

        anyhow::ensure!(
            row.get::<Option<String>>("used_at")?.is_none(),
            "Device link token is invalid, expired, or already used"
        );
        let expires_at: String = row.get("expires_at")?;
        let expires_at = time::OffsetDateTime::parse(
            &expires_at,
            &time::format_description::well_known::Rfc3339,
        )
        .context("invalid device link token expiry")?;
        anyhow::ensure!(
            expires_at > time::OffsetDateTime::now_utc(),
            "Device link token is invalid, expired, or already used"
        );
        anyhow::ensure!(
            row.get::<Option<String>>("disabled_at")?.is_none(),
            "Device link token account is disabled"
        );
        anyhow::ensure!(
            row.get::<Option<String>>("activated_at")?.is_some(),
            "Device link token account is not active"
        );

        let key_id = id();
        let account_id: String = row.get("account_id")?;
        let label: Option<String> = row.get("label")?;
        query(
            "INSERT INTO ssh_keys (id, account_id, fingerprint, public_key, label, created_at, last_used_at)
             VALUES (?, ?, ?, ?, ?, ?, ?)",
        )
        .bind(&key_id)
        .bind(&account_id)
        .bind(fingerprint)
        .bind(public_key)
        .bind(label.as_deref())
        .bind(&now)
        .bind(&now)
        .execute(&mut tx)
        .await
        .with_context(|| format!("linking key {fingerprint}"))?;
        let token_id: String = row.get("id")?;
        let updated = query(
            "UPDATE device_link_tokens
             SET used_at = ?, used_by_key_id = ?
             WHERE id = ? AND used_at IS NULL",
        )
        .bind(&now)
        .bind(&key_id)
        .bind(&token_id)
        .execute(&mut tx)
        .await?
        .rows_affected();
        anyhow::ensure!(
            updated == 1,
            "Device link token is invalid, expired, or already used"
        );
        query("UPDATE accounts SET last_seen_at = ?, updated_at = ? WHERE id = ?")
            .bind(&now)
            .bind(&now)
            .bind(&account_id)
            .execute(&mut tx)
            .await?;
        insert_audit(
            &mut tx,
            Some(&account_id),
            "ssh_key.linked",
            Some(&key_id),
            serde_json::json!({"username": row.get::<String>("username")?, "fingerprint": fingerprint}),
        )
        .await?;
        insert_event(
            &mut tx,
            None,
            None,
            None,
            "ssh_key.linked",
            serde_json::json!({"key_id": key_id, "account_id": account_id.clone()}),
        )
        .await?;
        let account = Account {
            id: account_id,
            username: row.get("username")?,
            display_name: row.get("display_name")?,
            role: Role::from_db(row.try_get::<String>("role")?.as_str())?,
            activated: true,
            pending_username: None,
        };
        tx.commit().await?;
        Ok(Some(account))
    }

    /// Redeem a bootstrap or invite token to create a pending account and bind
    /// the supplied SSH key. Username selection and activation happen inside the
    /// TUI after SSH auth succeeds.
    pub async fn redeem_token_for_key(
        &self,
        desired_username: &str,
        token: &str,
        fingerprint: &str,
        public_key: &str,
    ) -> anyhow::Result<Account> {
        let mut tx = self.db.write_pool().begin().await?;
        let now = now();
        let pending_username = match normalize_username(desired_username) {
            Ok(username) => {
                let existing: Option<String> =
                    query_scalar("SELECT id FROM accounts WHERE lower(username) = lower(?)")
                        .bind(&username)
                        .fetch_optional(&mut tx)
                        .await?;
                if existing.is_none() {
                    Some(username)
                } else {
                    None
                }
            }
            Err(_) => None,
        };
        let token_hash = code_hash(token);
        let active_count: i64 = query_scalar(
            "SELECT COUNT(*)
             FROM accounts
             WHERE activated_at IS NOT NULL AND disabled_at IS NULL",
        )
        .fetch_one(&mut tx)
        .await?;
        let account_id = id();
        let mut bootstrap_token_id = None;
        let mut invite_id = None;
        let role = if active_count == 0 {
            let token_id: Option<String> = query_scalar(
                "SELECT id
                 FROM bootstrap_tokens
                 WHERE code_hash = ? AND used_at IS NULL
                 LIMIT 1",
            )
            .bind(&token_hash)
            .fetch_optional(&mut tx)
            .await?;
            let Some(token_id) = token_id else {
                bail!("Bootstrap token is invalid or already used");
            };
            bootstrap_token_id = Some(token_id);
            Role::Owner
        } else {
            let invite = query(
                "SELECT id, role_on_accept
                 FROM invites
                 WHERE code_hash = ?
                   AND accepted_at IS NULL
                   AND revoked_at IS NULL
                   AND (expires_at IS NULL OR expires_at > ?)
                 LIMIT 1",
            )
            .bind(&token_hash)
            .bind(&now)
            .fetch_optional(&mut tx)
            .await?;
            let Some(invite) = invite else {
                bail!("Invite token is invalid, expired, or already used");
            };
            invite_id = Some(invite.get::<String>("id")?);
            Role::from_db(invite.get::<String>("role_on_accept")?.as_str())?
        };

        let username = pending_account_username(&account_id);
        query(
            "INSERT INTO accounts
             (id, username, display_name, role, settings_json, created_at, updated_at, last_seen_at, activated_at, pending_username)
             VALUES (?, ?, ?, ?, '{}', ?, ?, NULL, NULL, ?)",
        )
        .bind(&account_id)
        .bind(&username)
        .bind(&username)
        .bind(role.as_str())
        .bind(&now)
        .bind(&now)
        .bind(pending_username.as_deref())
        .execute(&mut tx)
        .await?;
        query(
            "INSERT INTO ssh_keys (id, account_id, fingerprint, public_key, label, created_at, last_used_at)
             VALUES (?, ?, ?, ?, 'default', ?, ?)",
        )
        .bind(id())
        .bind(&account_id)
        .bind(fingerprint)
        .bind(public_key)
        .bind(&now)
        .bind(&now)
        .execute(&mut tx)
        .await?;

        if let Some(token_id) = bootstrap_token_id {
            query(
                "UPDATE bootstrap_tokens
                 SET used_by_account_id = ?, used_at = ?
                 WHERE id = ? AND used_at IS NULL",
            )
            .bind(&account_id)
            .bind(&now)
            .bind(&token_id)
            .execute(&mut tx)
            .await?;
        }
        if let Some(invite_id) = invite_id {
            query(
                "UPDATE invites
                 SET accepted_by_account_id = ?, accepted_at = ?
                 WHERE id = ? AND accepted_at IS NULL",
            )
            .bind(&account_id)
            .bind(&now)
            .bind(&invite_id)
            .execute(&mut tx)
            .await?;
        }

        tx.commit().await?;
        Ok(Account {
            id: account_id,
            username,
            display_name: pending_username
                .clone()
                .unwrap_or_else(|| "pending".to_string()),
            role,
            activated: false,
            pending_username,
        })
    }

    pub async fn complete_onboarding(
        &self,
        account_id: &str,
        username: &str,
    ) -> anyhow::Result<Account> {
        let mut tx = self.db.write_pool().begin().await?;
        let now = now();
        let username = normalize_username(username)?;
        let row = query(
            "SELECT id, username, display_name, role, activated_at, pending_username
             FROM accounts
             WHERE id = ? AND disabled_at IS NULL",
        )
        .bind(account_id)
        .fetch_one(&mut tx)
        .await?;
        let role = Role::from_db(row.get::<String>("role")?.as_str())?;
        if row.get::<Option<String>>("activated_at")?.is_some() {
            tx.commit().await?;
            return account_from_row(row);
        }
        let existing: Option<String> =
            query_scalar("SELECT id FROM accounts WHERE lower(username) = lower(?) AND id <> ?")
                .bind(&username)
                .bind(account_id)
                .fetch_optional(&mut tx)
                .await?;
        anyhow::ensure!(existing.is_none(), "Username is already taken");
        if role == Role::Owner {
            let token_count: i64 = query_scalar(
                "SELECT COUNT(*)
                 FROM bootstrap_tokens
                 WHERE used_by_account_id = ? AND used_at IS NOT NULL",
            )
            .bind(account_id)
            .fetch_one(&mut tx)
            .await?;
            anyhow::ensure!(
                token_count > 0,
                "Pending account has no accepted login token"
            );
            let active_count: i64 = query_scalar(
                "SELECT COUNT(*)
                 FROM accounts
                 WHERE activated_at IS NOT NULL AND disabled_at IS NULL",
            )
            .fetch_one(&mut tx)
            .await?;
            anyhow::ensure!(
                active_count == 0,
                "Bootstrap token is invalid or already used"
            );
        } else {
            let token_count: i64 = query_scalar(
                "SELECT COUNT(*)
                 FROM invites
                 WHERE accepted_by_account_id = ? AND accepted_at IS NOT NULL",
            )
            .bind(account_id)
            .fetch_one(&mut tx)
            .await?;
            anyhow::ensure!(
                token_count > 0,
                "Pending account has no accepted login token"
            );
        }

        query(
            "UPDATE accounts
             SET username = ?, display_name = ?, last_seen_at = ?, activated_at = ?, updated_at = ?, pending_username = NULL
             WHERE id = ? AND activated_at IS NULL",
        )
        .bind(&username)
        .bind(&username)
        .bind(&now)
        .bind(&now)
        .bind(&now)
        .bind(account_id)
        .execute(&mut tx)
        .await?;
        query(
            "UPDATE ssh_keys
             SET last_used_at = ?
             WHERE account_id = ? AND revoked_at IS NULL",
        )
        .bind(&now)
        .bind(account_id)
        .execute(&mut tx)
        .await?;

        let general_id = if let Some(general_id) =
            query_scalar::<String>("SELECT id FROM channels WHERE slug = 'general'")
                .fetch_optional(&mut tx)
                .await?
        {
            general_id
        } else {
            let channel_id = id();
            query(
                    "INSERT INTO channels
                     (id, slug, name, visibility, topic, created_by_account_id, created_at, updated_at)
                     VALUES (?, 'general', 'general', 'public', 'General discussion', ?, ?, ?)",
                )
                .bind(&channel_id)
                .bind(account_id)
                .bind(&now)
                .bind(&now)
                .execute(&mut tx)
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
            channel_id
        };
        query(
            "INSERT INTO channel_members (channel_id, account_id, role, joined_at)
             VALUES (?, ?, ?, ?)
             ON CONFLICT(channel_id, account_id) DO NOTHING",
        )
        .bind(&general_id)
        .bind(account_id)
        .bind(if role == Role::Owner {
            "owner"
        } else {
            "member"
        })
        .bind(&now)
        .execute(&mut tx)
        .await?;

        if role == Role::Owner {
            insert_event(
                &mut tx,
                None,
                None,
                None,
                "account.bootstrapped",
                serde_json::json!({"account_id": account_id, "username": username}),
            )
            .await?;
        } else {
            insert_event(
                &mut tx,
                None,
                None,
                None,
                "invite.accepted",
                serde_json::json!({"account_id": account_id, "username": username}),
            )
            .await?;
        }

        tx.commit().await?;
        Ok(Account {
            id: account_id.to_string(),
            username: username.clone(),
            display_name: username,
            role,
            activated: true,
            pending_username: None,
        })
    }

    pub async fn reload_account(&self, account_id: &str) -> anyhow::Result<Account> {
        let row = query(
            "SELECT id, username, display_name, role, activated_at, pending_username
             FROM accounts WHERE id = ? AND disabled_at IS NULL",
        )
        .bind(account_id)
        .fetch_one(self.db.read_pool())
        .await?;
        account_from_row(row)
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
        let started = std::time::Instant::now();
        let history_limit = history_limit.clamp(1, MAX_HISTORY_LIMIT);
        let read_session = self.db.read_session().await?;
        let account = {
            let row = query(
                "SELECT id, username, display_name, role, activated_at, pending_username
                 FROM accounts WHERE id = ? AND disabled_at IS NULL",
            )
            .bind(account_id)
            .fetch_one(&read_session)
            .await?;
            account_from_row(row)?
        };
        if !account.activated {
            return Ok(Snapshot::default());
        }

        let channels = load_channels(&read_session, account_id).await?;
        let mut active_account_ids = load_active_presence_sessions(&read_session).await?;
        active_account_ids.extend(self.active_account_ids().await);
        let users = load_user_presence(&read_session, &active_account_ids).await?;
        let selected_channel_id = selected_channel_id
            .filter(|id| channels.iter().any(|channel| channel.id == *id))
            .map(ToOwned::to_owned)
            .or_else(|| channels.first().map(|channel| channel.id.clone()));

        let threads = if let Some(channel_id) = selected_channel_id.as_deref() {
            load_threads(&read_session, account_id, channel_id).await?
        } else {
            Vec::new()
        };
        let selected_thread_id = selected_thread_id
            .filter(|id| threads.iter().any(|thread| thread.id == *id))
            .map(ToOwned::to_owned)
            .or_else(|| threads.first().map(|thread| thread.id.clone()));
        let (comments, comments_has_more) = if let Some(thread_id) = selected_thread_id.as_deref() {
            load_comments(&read_session, account_id, thread_id, history_limit).await?
        } else {
            (Vec::new(), false)
        };

        let conversations = load_conversations(&read_session, account_id).await?;
        let dm_sidebar = load_dm_sidebar(&read_session, account_id).await?;
        let saved_count = load_saved_message_count(&read_session, account_id).await?;
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
            load_conversation_messages(&read_session, account_id, conversation_id, history_limit)
                .await?
        } else {
            (Vec::new(), false)
        };
        let notifications_page =
            load_notifications_page(&read_session, account_id, PageRequest::first(20)).await?;
        let unread_count_sql = format!(
            "SELECT
               (SELECT COUNT(*)
                FROM notifications n
                WHERE n.account_id = ? AND n.read_at IS NULL AND n.archived_at IS NULL AND {}) AS notification_unread_count,
               (SELECT COUNT(*)
                FROM mentions m
                WHERE m.target_account_id = ? AND m.read_at IS NULL AND {}) AS mention_unread_count",
            notification_visible_source_sql("n"),
            mention_visible_source_sql("m")
        );
        let unread_count_row = query(&unread_count_sql)
            .bind(account_id)
            .bind(account_id)
            .fetch_one(&read_session)
            .await?;
        let notification_unread_count: i64 = unread_count_row.get("notification_unread_count")?;
        let mention_unread_count: i64 = unread_count_row.get("mention_unread_count")?;

        let snapshot = Snapshot {
            current_username: Some(account.username),
            users,
            channels,
            threads,
            comments,
            conversations,
            dm_sidebar,
            conversation_messages,
            comments_has_more,
            conversation_messages_has_more,
            search_query: None,
            search_results: Vec::new(),
            search_next_cursor: None,
            search_has_more: false,
            saved_messages: Vec::new(),
            saved_next_cursor: None,
            saved_count,
            saved_has_more: false,
            notifications: notifications_page.items,
            notifications_next_cursor: notifications_page.next_cursor,
            notification_unread_count,
            mention_unread_count,
            selected_channel_id,
            selected_thread_id,
            selected_conversation_id,
        };
        tracing::debug!(
            elapsed_ms = started.elapsed().as_millis() as u64,
            channels = snapshot.channels.len(),
            threads = snapshot.threads.len(),
            comments = snapshot.comments.len(),
            conversations = snapshot.conversations.len(),
            dm_sidebar = snapshot.dm_sidebar.len(),
            conversation_messages = snapshot.conversation_messages.len(),
            notifications = snapshot.notifications.len(),
            saved_count = snapshot.saved_count,
            "snapshot loaded"
        );
        Ok(snapshot)
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
        let username: Option<String> = query_scalar(
            "SELECT username FROM accounts
             WHERE id = ? AND activated_at IS NOT NULL AND disabled_at IS NULL",
        )
        .bind(account_id)
        .fetch_optional(&mut tx)
        .await?;
        let Some(username) = username else {
            tx.commit().await?;
            return Ok(());
        };
        let now = now();
        query("UPDATE accounts SET last_seen_at = ?, updated_at = ? WHERE id = ?")
            .bind(&now)
            .bind(&now)
            .bind(account_id)
            .execute(&mut tx)
            .await?;
        if let Some(session_id) = session_id {
            if disconnected {
                query(
                    "UPDATE presence_sessions
                     SET last_seen_at = ?, disconnected_at = COALESCE(disconnected_at, ?)
                     WHERE id = ? AND account_id = ?",
                )
                .bind(&now)
                .bind(&now)
                .bind(session_id)
                .bind(account_id)
                .execute(&mut tx)
                .await?;
            } else {
                query(
                    "INSERT INTO presence_sessions (id, account_id, started_at, last_seen_at)
                     VALUES (?, ?, ?, ?)
                     ON CONFLICT(id) DO UPDATE SET last_seen_at = excluded.last_seen_at
                     WHERE presence_sessions.disconnected_at IS NULL",
                )
                .bind(session_id)
                .bind(account_id)
                .bind(&now)
                .bind(&now)
                .execute(&mut tx)
                .await?;
            }
        }
        insert_event(
            &mut tx,
            None,
            None,
            None,
            "presence.updated",
            serde_json::json!({"account_id": account_id, "username": username}),
        )
        .await?;
        tx.commit().await?;
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
        query_scalar(
            "SELECT id
             FROM presence_sessions
             WHERE account_id = ? AND disconnected_at IS NULL
             ORDER BY last_seen_at DESC, started_at DESC
             LIMIT 1",
        )
        .bind(account_id)
        .fetch_optional(self.db.read_pool())
        .await
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
        create_invite(self.db.write_pool(), &actor_id).await
    }

    pub async fn accept_invite(
        &self,
        account_id: String,
        code: String,
        username: String,
    ) -> anyhow::Result<()> {
        let _ = code;
        self.complete_onboarding(&account_id, &username).await?;
        Ok(())
    }

    pub async fn create_channel(
        &self,
        actor_id: String,
        name: String,
        private: bool,
    ) -> anyhow::Result<String> {
        create_channel(self.db.write_pool(), &actor_id, &name, private).await
    }

    pub async fn join_channel(&self, actor_id: String, slug: String) -> anyhow::Result<String> {
        join_channel(self.db.write_pool(), &actor_id, &slug).await
    }

    pub async fn create_thread(
        &self,
        actor_id: String,
        channel_id: String,
        title: String,
    ) -> anyhow::Result<String> {
        create_thread(self.db.write_pool(), &actor_id, &channel_id, &title).await
    }

    pub async fn add_comment(
        &self,
        actor_id: String,
        thread_id: String,
        body: String,
    ) -> anyhow::Result<()> {
        add_comment(self.db.write_pool(), &actor_id, &thread_id, &body).await
    }

    pub async fn open_dm(&self, actor_id: String, target: String) -> anyhow::Result<String> {
        open_dm(self.db.write_pool(), &actor_id, &target).await
    }

    pub async fn send_dm(
        &self,
        actor_id: String,
        conversation_id: String,
        body: String,
    ) -> anyhow::Result<()> {
        send_dm(self.db.write_pool(), &actor_id, &conversation_id, &body).await
    }
}
