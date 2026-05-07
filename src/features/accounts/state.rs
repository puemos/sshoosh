use super::*;

const DEVICE_LINK_TOKEN_TTL_MINUTES: i64 = 10;

impl ServerState {
    pub async fn create_invite_with_options(
        &self,
        actor_id: &str,
        role_on_accept: Role,
        ttl_hours: Option<i64>,
    ) -> anyhow::Result<String> {
        create_invite_with_options(self.db.write_pool(), actor_id, role_on_accept, ttl_hours).await
    }

    pub async fn list_accounts(&self, actor_id: &str) -> anyhow::Result<Vec<AccountSummary>> {
        Ok(self
            .list_accounts_page(actor_id, PageRequest::first(500))
            .await?
            .items)
    }

    pub async fn list_accounts_page(
        &self,
        actor_id: &str,
        request: PageRequest,
    ) -> anyhow::Result<Page<AccountSummary>> {
        let mut tx = begin(self.db.write_pool()).await?;
        require_admin_tx(&mut tx, actor_id).await?;
        let limit = page_limit(request.limit, 500);
        let cursor = decode_cursor(request.cursor.as_deref(), 2)?;
        let cursor_filter = if cursor.is_some() {
            "WHERE lower(username) > ? OR (lower(username) = ? AND id > ?)"
        } else {
            ""
        };
        let sql = format!(
            "SELECT id, username, lower(username) AS lower_username, display_name, role,
                    activated_at, disabled_at, created_at, last_seen_at
             FROM accounts
             {cursor_filter}
             ORDER BY lower(username), id
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
        let mut items: Vec<AccountSummary> = Vec::new();
        let mut next_cursor = None;
        for (idx, row) in rows.into_iter().enumerate() {
            if idx == limit as usize {
                let last = items.last().expect("last account row");
                next_cursor = Some(encode_cursor([
                    last.username.to_lowercase(),
                    last.id.clone(),
                ])?);
                break;
            }
            let row_display_name = row.try_get::<String>("display_name")?;
            let row_role = row.try_get::<String>("role")?;
            let id: String = row.get("id")?;
            let username: String = row.get("username")?;
            let created_at: String = row.get("created_at")?;
            let last_seen_at: Option<String> = row.get("last_seen_at")?;
            items.push(AccountSummary {
                id,
                username,
                display_name: sanitize_single_line_text(&row_display_name),
                role: Role::from_db(row_role.as_str())?,
                activated: row.try_get::<Option<String>>("activated_at")?.is_some(),
                disabled: row.try_get::<Option<String>>("disabled_at")?.is_some(),
                created_at,
                last_seen_at,
            });
        }
        Ok(Page { items, next_cursor })
    }

    pub async fn set_user_disabled(
        &self,
        actor_id: &str,
        username: &str,
        disabled: bool,
    ) -> anyhow::Result<()> {
        let mut tx = begin(self.db.write_pool()).await?;
        let actor = require_admin_tx(&mut tx, actor_id).await?;
        let target = load_account_by_username_tx(&mut tx, username).await?;
        ensure_can_manage_account(&actor, &target)?;
        if disabled && target.role == Role::Owner {
            ensure_not_last_active_owner(&mut tx, &target.id).await?;
        }
        let now = now();
        query("UPDATE accounts SET disabled_at = ?, updated_at = ? WHERE id = ?")
            .bind(if disabled { Some(now.clone()) } else { None })
            .bind(&now)
            .bind(&target.id)
            .execute(&mut tx)
            .await?;
        insert_audit(
            &mut tx,
            Some(actor_id),
            if disabled {
                "user.disabled"
            } else {
                "user.enabled"
            },
            Some(&target.id),
            serde_json::json!({"username": target.username}),
        )
        .await?;
        insert_event(
            &mut tx,
            None,
            None,
            None,
            if disabled {
                "user.disabled"
            } else {
                "user.enabled"
            },
            serde_json::json!({"account_id": target.id, "username": target.username}),
        )
        .await?;
        tx.commit().await?;
        Ok(())
    }

    pub async fn set_user_role(
        &self,
        actor_id: &str,
        username: &str,
        role: Role,
    ) -> anyhow::Result<()> {
        let mut tx = begin(self.db.write_pool()).await?;
        let actor = require_admin_tx(&mut tx, actor_id).await?;
        let target = load_account_by_username_tx(&mut tx, username).await?;
        ensure_can_manage_account(&actor, &target)?;
        if actor.role != Role::Owner && matches!(role, Role::Owner | Role::Admin) {
            bail!("Only owners can grant owner/admin roles");
        }
        if target.role == Role::Owner && role != Role::Owner {
            ensure_not_last_active_owner(&mut tx, &target.id).await?;
        }
        let now = now();
        query("UPDATE accounts SET role = ?, updated_at = ? WHERE id = ?")
            .bind(role.as_str())
            .bind(&now)
            .bind(&target.id)
            .execute(&mut tx)
            .await?;
        insert_audit(
            &mut tx,
            Some(actor_id),
            "user.role_changed",
            Some(&target.id),
            serde_json::json!({"username": target.username, "role": role.as_str()}),
        )
        .await?;
        insert_event(
            &mut tx,
            None,
            None,
            None,
            "user.role_changed",
            serde_json::json!({"account_id": target.id, "username": target.username, "role": role.as_str()}),
        )
        .await?;
        tx.commit().await?;
        Ok(())
    }

    pub async fn rename_user(
        &self,
        actor_id: &str,
        username: &str,
        next_username: &str,
    ) -> anyhow::Result<()> {
        let next_username = normalize_username(next_username)?;
        let mut tx = begin(self.db.write_pool()).await?;
        let actor = load_account_tx(&mut tx, actor_id).await?;
        let target = if username == actor_id {
            actor.clone()
        } else {
            load_account_by_username_tx(&mut tx, username).await?
        };
        if actor.id != target.id {
            ensure_can_manage_account(&actor, &target)?;
        }
        ensure_username_reservable_tx(&mut tx, &target.id, &next_username).await?;
        let now = now();
        query("UPDATE accounts SET username = ?, updated_at = ? WHERE id = ?")
            .bind(&next_username)
            .bind(&now)
            .bind(&target.id)
            .execute(&mut tx)
            .await?;
        set_current_username_reservation_tx(&mut tx, &target.id, &next_username, &now).await?;
        insert_audit(
            &mut tx,
            Some(actor_id),
            "user.renamed",
            Some(&target.id),
            serde_json::json!({"from": target.username, "to": next_username}),
        )
        .await?;
        insert_event(
            &mut tx,
            None,
            None,
            None,
            "user.renamed",
            serde_json::json!({"account_id": target.id, "username": next_username}),
        )
        .await?;
        tx.commit().await?;
        Ok(())
    }

    pub async fn set_display_name(
        &self,
        actor_id: &str,
        username: &str,
        display_name: &str,
    ) -> anyhow::Result<()> {
        let display_name = sanitize_single_line_text(display_name);
        let display_name = display_name.trim();
        anyhow::ensure!(
            (1..=80).contains(&display_name.chars().count()),
            "Display name must be 1-80 characters"
        );
        let mut tx = begin(self.db.write_pool()).await?;
        let actor = load_account_tx(&mut tx, actor_id).await?;
        let target = if username == actor_id {
            actor.clone()
        } else {
            load_account_by_username_tx(&mut tx, username).await?
        };
        if actor.id != target.id {
            ensure_can_manage_account(&actor, &target)?;
        }
        let now = now();
        query("UPDATE accounts SET display_name = ?, updated_at = ? WHERE id = ?")
            .bind(display_name)
            .bind(&now)
            .bind(&target.id)
            .execute(&mut tx)
            .await?;
        insert_audit(
            &mut tx,
            Some(actor_id),
            "user.display_name_changed",
            Some(&target.id),
            serde_json::json!({"username": target.username, "display_name": display_name}),
        )
        .await?;
        insert_event(
            &mut tx,
            None,
            None,
            None,
            "user.display_name_changed",
            serde_json::json!({"account_id": target.id, "display_name": display_name}),
        )
        .await?;
        tx.commit().await?;
        Ok(())
    }

    pub async fn list_my_ssh_keys(&self, account_id: &str) -> anyhow::Result<Vec<SshKeySummary>> {
        load_my_ssh_keys(self.db.read_pool(), account_id).await
    }

    pub async fn create_device_link_token(
        &self,
        account_id: &str,
        label: Option<&str>,
    ) -> anyhow::Result<String> {
        let mut tx = begin(self.db.write_pool()).await?;
        let account = load_account_tx(&mut tx, account_id).await?;
        anyhow::ensure!(account.activated, "Account is not active");
        let token = invite_code();
        let token_hash = code_hash(&token);
        let now = now();
        let expires_at = (time::OffsetDateTime::now_utc()
            + time::Duration::minutes(DEVICE_LINK_TOKEN_TTL_MINUTES))
        .format(&time::format_description::well_known::Rfc3339)?;
        let label = label
            .map(sanitize_single_line_text)
            .map(|value| value.trim().to_string())
            .filter(|value| !value.is_empty());
        let token_id = id();
        query(
            "INSERT INTO device_link_tokens
             (id, account_id, code_hash, label, created_at, expires_at)
             VALUES (?, ?, ?, ?, ?, ?)",
        )
        .bind(&token_id)
        .bind(&account.id)
        .bind(&token_hash)
        .bind(label.as_deref())
        .bind(&now)
        .bind(&expires_at)
        .execute(&mut tx)
        .await?;
        insert_audit(
            &mut tx,
            Some(account_id),
            "device_link_token.created",
            Some(&token_id),
            serde_json::json!({"username": account.username, "expires_at": expires_at}),
        )
        .await?;
        tx.commit().await?;
        Ok(token)
    }

    pub async fn add_ssh_key(
        &self,
        actor_id: &str,
        username: Option<&str>,
        public_key: &str,
        label: Option<&str>,
    ) -> anyhow::Result<SshKeySummary> {
        let parsed = parse_public_key(public_key)?;
        let mut tx = begin(self.db.write_pool()).await?;
        let actor = load_account_tx(&mut tx, actor_id).await?;
        let target = if let Some(username) = username {
            let target = load_account_by_username_tx(&mut tx, username).await?;
            if actor.id != target.id {
                ensure_can_manage_account(&actor, &target)?;
            }
            target
        } else {
            actor.clone()
        };
        let now = now();
        let key_id = id();
        let label = label
            .map(sanitize_single_line_text)
            .map(|value| value.trim().to_string())
            .filter(|value| !value.is_empty());
        query(
            "INSERT INTO ssh_keys (id, account_id, fingerprint, public_key, label, created_at)
             VALUES (?, ?, ?, ?, ?, ?)",
        )
        .bind(&key_id)
        .bind(&target.id)
        .bind(&parsed.fingerprint)
        .bind(&parsed.public_key)
        .bind(label.as_deref())
        .bind(&now)
        .execute(&mut tx)
        .await
        .with_context(|| format!("adding key {}", parsed.fingerprint))?;
        insert_audit(
            &mut tx,
            Some(actor_id),
            "ssh_key.added",
            Some(&key_id),
            serde_json::json!({"username": target.username, "fingerprint": parsed.fingerprint}),
        )
        .await?;
        insert_event(
            &mut tx,
            None,
            None,
            None,
            "ssh_key.added",
            serde_json::json!({"key_id": key_id, "account_id": target.id}),
        )
        .await?;
        tx.commit().await?;
        Ok(SshKeySummary {
            id: key_id,
            username: target.username,
            fingerprint: parsed.fingerprint,
            label,
            created_at: now,
            last_used_at: None,
            revoked_at: None,
        })
    }

    pub async fn label_ssh_key(
        &self,
        actor_id: &str,
        key: &str,
        label: &str,
    ) -> anyhow::Result<()> {
        let mut tx = begin(self.db.write_pool()).await?;
        let actor = load_account_tx(&mut tx, actor_id).await?;
        let row = query(
            "SELECT k.id, k.account_id, a.username, a.role
             FROM ssh_keys k
             JOIN accounts a ON a.id = k.account_id
             WHERE (k.id LIKE ? OR k.fingerprint = ?) AND k.revoked_at IS NULL",
        )
        .bind(format!("{}%", key.trim()))
        .bind(key.trim())
        .fetch_optional(&mut tx)
        .await?;
        let Some(row) = row else {
            bail!("Active SSH key not found");
        };
        let target = Account {
            id: row.get("account_id")?,
            username: row.get("username")?,
            display_name: String::new(),
            role: Role::from_db(row.try_get::<String>("role")?.as_str())?,
            activated: true,
            pending_username: None,
        };
        if actor.id != target.id {
            ensure_can_manage_account(&actor, &target)?;
        }
        let key_id: String = row.get("id")?;
        let label = sanitize_single_line_text(label);
        let label = label.trim();
        let label = (!label.is_empty()).then_some(label);
        query("UPDATE ssh_keys SET label = ? WHERE id = ?")
            .bind(label)
            .bind(&key_id)
            .execute(&mut tx)
            .await?;
        insert_audit(
            &mut tx,
            Some(actor_id),
            "ssh_key.labeled",
            Some(&key_id),
            serde_json::json!({"username": target.username, "label": label}),
        )
        .await?;
        tx.commit().await?;
        Ok(())
    }

    pub async fn list_ssh_keys(&self, actor_id: &str) -> anyhow::Result<Vec<SshKeySummary>> {
        Ok(self
            .list_ssh_keys_page(actor_id, PageRequest::first(500))
            .await?
            .items)
    }

    pub async fn list_ssh_keys_page(
        &self,
        actor_id: &str,
        request: PageRequest,
    ) -> anyhow::Result<Page<SshKeySummary>> {
        let mut tx = begin(self.db.write_pool()).await?;
        require_admin_tx(&mut tx, actor_id).await?;
        let limit = page_limit(request.limit, 500);
        let cursor = decode_cursor(request.cursor.as_deref(), 3)?;
        let cursor_filter = if cursor.is_some() {
            "WHERE lower(a.username) > ?
                OR (lower(a.username) = ? AND k.created_at > ?)
                OR (lower(a.username) = ? AND k.created_at = ? AND k.id > ?)"
        } else {
            ""
        };
        let sql = format!(
            "SELECT k.id, a.username, lower(a.username) AS lower_username, k.fingerprint,
                    k.label, k.created_at, k.last_used_at, k.revoked_at
             FROM ssh_keys k
             JOIN accounts a ON a.id = k.account_id
             {cursor_filter}
             ORDER BY lower(a.username), k.created_at, k.id
             LIMIT ?"
        );
        let mut query = query(&sql);
        if let Some(cursor) = cursor {
            query = query
                .bind(&cursor[0])
                .bind(&cursor[0])
                .bind(&cursor[1])
                .bind(&cursor[0])
                .bind(&cursor[1])
                .bind(&cursor[2]);
        }
        let rows = query
            .bind(limit.saturating_add(1))
            .fetch_all(&mut tx)
            .await?;
        tx.commit().await?;
        let mut items: Vec<SshKeySummary> = Vec::new();
        let mut next_cursor = None;
        for (idx, row) in rows.into_iter().enumerate() {
            if idx == limit as usize {
                let last = items.last().expect("last SSH key row");
                next_cursor = Some(encode_cursor([
                    last.username.to_lowercase(),
                    last.created_at.clone(),
                    last.id.clone(),
                ])?);
                break;
            }
            items.push(ssh_key_summary_from_row(row)?);
        }
        Ok(Page { items, next_cursor })
    }

    pub async fn revoke_ssh_key(&self, actor_id: &str, key: &str) -> anyhow::Result<()> {
        let mut tx = begin(self.db.write_pool()).await?;
        let actor = load_account_tx(&mut tx, actor_id).await?;
        let row = query(
            "SELECT k.id, k.account_id, k.fingerprint, a.username, a.role
             FROM ssh_keys k
             JOIN accounts a ON a.id = k.account_id
             WHERE (k.id LIKE ? OR k.fingerprint = ?) AND k.revoked_at IS NULL",
        )
        .bind(format!("{}%", key.trim()))
        .bind(key.trim())
        .fetch_optional(&mut tx)
        .await?;
        let Some(row) = row else {
            bail!("Active SSH key not found");
        };
        let target = Account {
            id: row.get("account_id")?,
            username: row.get("username")?,
            display_name: String::new(),
            role: Role::from_db(row.try_get::<String>("role")?.as_str())?,
            activated: true,
            pending_username: None,
        };
        if actor.id != target.id {
            ensure_can_manage_account(&actor, &target)?;
        }
        if target.role == Role::Owner {
            ensure_owner_keeps_active_key(&mut tx, &target.id).await?;
        }
        let key_id: String = row.get("id")?;
        let now = now();
        query("UPDATE ssh_keys SET revoked_at = ? WHERE id = ?")
            .bind(&now)
            .bind(&key_id)
            .execute(&mut tx)
            .await?;
        insert_audit(
            &mut tx,
            Some(actor_id),
            "ssh_key.revoked",
            Some(&key_id),
            serde_json::json!({"username": target.username, "fingerprint": row.get::<String>("fingerprint")?}),
        )
        .await?;
        insert_event(
            &mut tx,
            None,
            None,
            None,
            "ssh_key.revoked",
            serde_json::json!({"key_id": key_id, "account_id": target.id}),
        )
        .await?;
        tx.commit().await?;
        Ok(())
    }
}
