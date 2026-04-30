use super::*;
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
        let mut tx = begin(self.db.write_pool()).await?;
        require_admin_tx(&mut tx, actor_id).await?;
        let rows = query(
            "SELECT id, username, display_name, role, activated_at, disabled_at, created_at, last_seen_at
             FROM accounts
             ORDER BY username",
        )
        .fetch_all(&mut tx)
        .await?;
        tx.commit().await?;
        rows.into_iter()
            .map(|row| {
                Ok(AccountSummary {
                    id: row.get("id"),
                    username: row.get("username"),
                    display_name: row.get("display_name"),
                    role: Role::from_db(row.get::<String>("role").as_str())?,
                    activated: row.get::<Option<String>>("activated_at").is_some(),
                    disabled: row.get::<Option<String>>("disabled_at").is_some(),
                    created_at: row.get("created_at"),
                    last_seen_at: row.get("last_seen_at"),
                })
            })
            .collect()
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
        if actor.role != Role::Owner && role == Role::Owner {
            bail!("Only owners can promote another owner");
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
        let existing: Option<String> =
            query_scalar("SELECT id FROM accounts WHERE lower(username) = lower(?) AND id <> ?")
                .bind(&next_username)
                .bind(&target.id)
                .fetch_optional(&mut tx)
                .await?;
        anyhow::ensure!(existing.is_none(), "Username is already taken");
        let now = now();
        query("UPDATE accounts SET username = ?, updated_at = ? WHERE id = ?")
            .bind(&next_username)
            .bind(&now)
            .bind(&target.id)
            .execute(&mut tx)
            .await?;
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
        let rows = query(
            "SELECT k.id, a.username, k.fingerprint, k.label, k.created_at, k.last_used_at, k.revoked_at
             FROM ssh_keys k
             JOIN accounts a ON a.id = k.account_id
             WHERE k.account_id = ?
             ORDER BY k.created_at",
        )
        .bind(account_id)
        .fetch_all(self.db.read_pool())
        .await?;
        Ok(rows.into_iter().map(ssh_key_summary_from_row).collect())
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
        query(
            "INSERT INTO ssh_keys (id, account_id, fingerprint, public_key, label, created_at)
             VALUES (?, ?, ?, ?, ?, ?)",
        )
        .bind(&key_id)
        .bind(&target.id)
        .bind(&parsed.fingerprint)
        .bind(&parsed.public_key)
        .bind(label.map(str::trim).filter(|value| !value.is_empty()))
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
            label: label
                .map(str::trim)
                .filter(|value| !value.is_empty())
                .map(ToOwned::to_owned),
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
            id: row.get("account_id"),
            username: row.get("username"),
            display_name: String::new(),
            role: Role::from_db(row.get::<String>("role").as_str())?,
            activated: true,
            pending_username: None,
        };
        if actor.id != target.id {
            ensure_can_manage_account(&actor, &target)?;
        }
        let key_id: String = row.get("id");
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

    pub async fn attach_ssh_key(
        &self,
        actor_id: &str,
        key: &str,
        username: &str,
    ) -> anyhow::Result<()> {
        let mut tx = begin(self.db.write_pool()).await?;
        require_admin_tx(&mut tx, actor_id).await?;
        let target = load_account_by_username_tx(&mut tx, username).await?;
        let row = query(
            "SELECT k.id, k.account_id, a.username AS old_username
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
        let key_id: String = row.get("id");
        let old_account_id: String = row.get("account_id");
        query("UPDATE ssh_keys SET account_id = ? WHERE id = ?")
            .bind(&target.id)
            .bind(&key_id)
            .execute(&mut tx)
            .await?;
        let remaining_keys: i64 = query_scalar(
            "SELECT COUNT(*) FROM ssh_keys WHERE account_id = ? AND revoked_at IS NULL",
        )
        .bind(&old_account_id)
        .fetch_one(&mut tx)
        .await?;
        if remaining_keys == 0 {
            let now = now();
            query("UPDATE accounts SET disabled_at = ?, updated_at = ? WHERE id = ? AND activated_at IS NULL")
                .bind(&now)
                .bind(&now)
                .bind(&old_account_id)
                .execute(&mut tx)
                .await?;
        }
        insert_audit(
            &mut tx,
            Some(actor_id),
            "ssh_key.attached",
            Some(&key_id),
            serde_json::json!({"from_account_id": old_account_id, "to_username": target.username}),
        )
        .await?;
        insert_event(
            &mut tx,
            None,
            None,
            None,
            "ssh_key.attached",
            serde_json::json!({"key_id": key_id, "account_id": target.id}),
        )
        .await?;
        tx.commit().await?;
        Ok(())
    }

    pub async fn list_ssh_keys(&self, actor_id: &str) -> anyhow::Result<Vec<SshKeySummary>> {
        let mut tx = begin(self.db.write_pool()).await?;
        require_admin_tx(&mut tx, actor_id).await?;
        let rows = query(
            "SELECT k.id, a.username, k.fingerprint, k.label, k.created_at, k.last_used_at, k.revoked_at
             FROM ssh_keys k
             JOIN accounts a ON a.id = k.account_id
             ORDER BY a.username, k.created_at",
        )
        .fetch_all(&mut tx)
        .await?;
        tx.commit().await?;
        Ok(rows.into_iter().map(ssh_key_summary_from_row).collect())
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
            id: row.get("account_id"),
            username: row.get("username"),
            display_name: String::new(),
            role: Role::from_db(row.get::<String>("role").as_str())?,
            activated: true,
            pending_username: None,
        };
        if actor.id != target.id {
            ensure_can_manage_account(&actor, &target)?;
        }
        if target.role == Role::Owner {
            ensure_owner_keeps_active_key(&mut tx, &target.id).await?;
        }
        let key_id: String = row.get("id");
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
            serde_json::json!({"username": target.username, "fingerprint": row.get::<String>("fingerprint")}),
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
