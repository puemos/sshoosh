use super::executor::{QueryMutation, normalize_sql, query_mutation, value_as_str};
use super::*;

pub(super) const ENVELOPE_PREFIX: &str = "sshoosh:v1:xchacha20poly1305:";

pub(super) struct EncryptionService {
    cipher: XChaCha20Poly1305,
}

impl EncryptionService {
    pub(super) fn from_base64url(key: &str) -> anyhow::Result<Self> {
        let bytes = Zeroizing::new(
            URL_SAFE_NO_PAD
                .decode(key)
                .context("SSHOOSH_ENCRYPTION_KEY must be base64url without padding")?,
        );
        anyhow::ensure!(
            bytes.len() == 32,
            "SSHOOSH_ENCRYPTION_KEY must decode to exactly 32 bytes"
        );
        let cipher = XChaCha20Poly1305::new_from_slice(&bytes)?;
        Ok(Self { cipher })
    }

    fn encrypt(
        &self,
        table: &str,
        row_id: &str,
        column: &str,
        plaintext: &str,
    ) -> anyhow::Result<String> {
        if plaintext.starts_with(ENVELOPE_PREFIX) {
            return Ok(plaintext.to_string());
        }
        let mut nonce = [0u8; 24];
        OsRng.fill_bytes(&mut nonce);
        let ciphertext = self.cipher.encrypt(
            XNonce::from_slice(&nonce),
            chacha20poly1305::aead::Payload {
                msg: plaintext.as_bytes(),
                aad: associated_data(table, row_id, column).as_bytes(),
            },
        )?;
        Ok(format!(
            "{ENVELOPE_PREFIX}{}:{}",
            URL_SAFE_NO_PAD.encode(nonce),
            URL_SAFE_NO_PAD.encode(ciphertext)
        ))
    }

    pub(super) fn decrypt(
        &self,
        table: &str,
        row_id: &str,
        column: &str,
        envelope: &str,
    ) -> anyhow::Result<String> {
        let body = envelope
            .strip_prefix(ENVELOPE_PREFIX)
            .context("invalid encryption envelope")?;
        let (nonce, ciphertext) = body
            .split_once(':')
            .context("invalid encryption envelope")?;
        let nonce = URL_SAFE_NO_PAD.decode(nonce)?;
        let ciphertext = URL_SAFE_NO_PAD.decode(ciphertext)?;
        let plaintext = self.cipher.decrypt(
            XNonce::from_slice(&nonce),
            chacha20poly1305::aead::Payload {
                msg: &ciphertext,
                aad: associated_data(table, row_id, column).as_bytes(),
            },
        )?;
        Ok(String::from_utf8(plaintext)?)
    }
}

impl Database {
    pub async fn encrypt_migrate(&self) -> anyhow::Result<EncryptionMigrationReport> {
        anyhow::ensure!(
            self.encryption.is_some(),
            "SSHOOSH_ENCRYPTION_KEY is required for encrypt migrate"
        );
        anyhow::ensure!(
            self.is_master(),
            "encrypt migrate requires this process to hold the master lease"
        );
        let mut tx = self.transaction_unchecked().await?;
        query(
            "INSERT INTO audit_log (id, actor_account_id, action, target, metadata_json, created_at)
             VALUES (?, NULL, 'encryption.migration_started', NULL, '{}', ?)",
        )
        .bind(random_id())
        .bind(now())
        .execute_unchecked(&mut tx)
        .await?;

        let report = EncryptionMigrationReport {
            threads: migrate_table_columns(&mut tx, "threads", &["title", "body"]).await?,
            comments: migrate_table_columns(&mut tx, "comments", &["body"]).await?,
            conversation_messages: migrate_table_columns(
                &mut tx,
                "conversation_messages",
                &["body"],
            )
            .await?,
            notifications: migrate_table_columns(&mut tx, "notifications", &["title", "body"])
                .await?,
        };

        query(
            "INSERT INTO audit_log (id, actor_account_id, action, target, metadata_json, created_at)
             VALUES (?, NULL, 'encryption.migration_completed', NULL, ?, ?)",
        )
        .bind(random_id())
        .bind(serde_json::to_string(&report)?)
        .bind(now())
        .execute_unchecked(&mut tx)
        .await?;
        tx.commit().await?;
        Ok(report)
    }

    pub(super) async fn validate_encryption(&self, allow_plaintext: bool) -> anyhow::Result<()> {
        let encrypted_count: i64 = query_scalar(
            "SELECT
               (SELECT COUNT(*) FROM threads WHERE title LIKE 'sshoosh:v1:xchacha20poly1305:%' OR body LIKE 'sshoosh:v1:xchacha20poly1305:%') +
               (SELECT COUNT(*) FROM comments WHERE body LIKE 'sshoosh:v1:xchacha20poly1305:%') +
               (SELECT COUNT(*) FROM conversation_messages WHERE body LIKE 'sshoosh:v1:xchacha20poly1305:%') +
               (SELECT COUNT(*) FROM notifications WHERE title LIKE 'sshoosh:v1:xchacha20poly1305:%' OR body LIKE 'sshoosh:v1:xchacha20poly1305:%')",
        )
        .fetch_one_unchecked(self)
        .await
        .unwrap_or(0);
        if encrypted_count > 0 && self.encryption.is_none() {
            bail!("encrypted content exists but SSHOOSH_ENCRYPTION_KEY is not configured");
        }
        if self.encryption.is_some() && !allow_plaintext {
            let plaintext_count: i64 = query_scalar(
                "SELECT
                   (SELECT COUNT(*) FROM threads WHERE (title NOT LIKE 'sshoosh:v1:xchacha20poly1305:%' OR body NOT LIKE 'sshoosh:v1:xchacha20poly1305:%')) +
                   (SELECT COUNT(*) FROM comments WHERE body NOT LIKE 'sshoosh:v1:xchacha20poly1305:%') +
                   (SELECT COUNT(*) FROM conversation_messages WHERE body NOT LIKE 'sshoosh:v1:xchacha20poly1305:%') +
                   (SELECT COUNT(*) FROM notifications WHERE title NOT LIKE 'sshoosh:v1:xchacha20poly1305:%' OR body NOT LIKE 'sshoosh:v1:xchacha20poly1305:%')",
            )
            .fetch_one_unchecked(self)
            .await
            .unwrap_or(0);
            if plaintext_count > 0 {
                bail!(
                    "plaintext content exists; run `sshoosh encrypt migrate` with SSHOOSH_ENCRYPTION_KEY"
                );
            }
        }
        if encrypted_count > 0 {
            let _ = query(
                "SELECT id, title, body
                 FROM threads
                 WHERE title LIKE 'sshoosh:v1:xchacha20poly1305:%' OR body LIKE 'sshoosh:v1:xchacha20poly1305:%'
                 LIMIT 1",
            )
            .fetch_optional_unchecked(self)
            .await?
            .map(|row| -> anyhow::Result<()> {
                let _: String = row.try_get("title")?;
                let _: String = row.try_get("body")?;
                Ok(())
            })
            .transpose()?;
        }
        Ok(())
    }
}

pub(super) fn encrypt_param(
    encryption: &EncryptionService,
    params: &mut [Value],
    value_idx: usize,
    table: &str,
    id_idx: usize,
    column: &str,
) -> anyhow::Result<()> {
    let Some(id) = params.get(id_idx).and_then(value_as_str) else {
        return Ok(());
    };
    let Some(Value::Text(value)) = params.get(value_idx).cloned() else {
        return Ok(());
    };
    params[value_idx] = Value::Text(encryption.encrypt(table, &id, column, &value)?);
    Ok(())
}

pub(super) fn encrypt_named_payload(
    encryption: &EncryptionService,
    params: &mut [Value],
    sql: &str,
    mutation: &Option<QueryMutation>,
) -> anyhow::Result<()> {
    let (id_idx, payload_idx) = match mutation {
        Some(QueryMutation::Insert { .. }) => locate_named_params(sql, "webhook_jobs", false),
        Some(QueryMutation::Update { .. }) => locate_named_params(sql, "webhook_jobs", true),
        _ => return Ok(()),
    };
    let Some(id_idx) = id_idx else {
        return Ok(());
    };
    let Some(payload_idx) = payload_idx else {
        return Ok(());
    };
    let Some(id) = params.get(id_idx).and_then(value_as_str) else {
        return Ok(());
    };
    let Some(Value::Text(payload)) = params.get(payload_idx).cloned() else {
        return Ok(());
    };
    params[payload_idx] =
        Value::Text(encryption.encrypt("webhook_jobs", &id, "payload_json", &payload)?);
    Ok(())
}

pub(super) fn encrypt_named_update_column(
    encryption: &EncryptionService,
    params: &mut [Value],
    sql: &str,
    table: &str,
    column: &str,
) -> anyhow::Result<()> {
    let normalized = normalize_sql(sql);
    if query_mutation(&normalized)
        != Some(QueryMutation::Update {
            table: table.to_string(),
        })
    {
        return Ok(());
    }
    let (id_idx, value_idx) = locate_update_indices(&normalized, column);
    let Some(id_idx) = id_idx else {
        return Ok(());
    };
    let Some(value_idx) = value_idx else {
        return Ok(());
    };
    let Some(id) = params.get(id_idx).and_then(value_as_str) else {
        return Ok(());
    };
    let Some(Value::Text(value)) = params.get(value_idx).cloned() else {
        return Ok(());
    };
    params[value_idx] = Value::Text(encryption.encrypt(table, &id, column, &value)?);
    Ok(())
}

fn locate_named_params(sql: &str, table: &str, is_update: bool) -> (Option<usize>, Option<usize>) {
    let normalized = normalize_sql(sql);
    if !normalized.contains(&format!(" {table}")) {
        return (None, None);
    }
    if is_update {
        return locate_update_indices(&normalized, "payload_json");
    }
    locate_webhook_insert_indices(&normalized)
}

fn locate_webhook_insert_indices(sql: &str) -> (Option<usize>, Option<usize>) {
    let values_start = sql.find(" values ");
    let Some(values_start) = values_start else {
        return (None, None);
    };
    let columns_part = sql
        .split_once("insert into webhook_jobs")
        .and_then(|(_, rest)| rest.split_once(" values "))
        .map(|(columns, _)| columns.trim());
    let Some(columns_part) = columns_part else {
        return (Some(0), Some(1));
    };
    let values_part = &sql[values_start + 8..];
    if values_part.is_empty() {
        return (None, None);
    }
    let id_idx = parse_sql_column_index(columns_part, "id");
    let payload_idx = parse_sql_column_index(columns_part, "payload_json")
        .or_else(|| Some(values_part.matches("?").count().saturating_sub(1)));
    (id_idx, payload_idx)
}

fn parse_sql_column_index(columns: &str, target: &str) -> Option<usize> {
    let (columns, _) = columns.trim().split_once('(')?;
    let (columns, _) = columns.rsplit_once(')')?;
    columns
        .split(',')
        .map(str::trim)
        .position(|column| column == target)
}

fn locate_update_indices(sql: &str, target_column: &str) -> (Option<usize>, Option<usize>) {
    let Some(set_start) = sql.find(" set ") else {
        return (None, None);
    };
    let Some(where_start) = sql.rfind(" where ") else {
        return (None, None);
    };
    let set_clause = &sql[set_start + 5..where_start];
    let where_clause = &sql[where_start + 7..];
    let mut payload_idx = None;
    let mut params_before_assignment = 0usize;
    let mut set_param_count = 0usize;
    for assignment in set_clause.split(',') {
        let assignment_param_count = assignment.matches('?').count();
        if assignment
            .split('=')
            .next()
            .map(str::trim)
            .is_some_and(|left| left == target_column)
        {
            payload_idx = Some(params_before_assignment);
        }
        params_before_assignment += assignment_param_count;
        set_param_count += assignment_param_count;
    }
    if payload_idx.is_none() {
        return (None, None);
    }

    let where_has_id = where_clause
        .split_whitespace()
        .zip(where_clause.split_whitespace().skip(1))
        .any(|(left, right)| left == "id" && right.trim_start().starts_with('='));
    let id_idx = where_has_id.then_some(set_param_count);
    (id_idx, payload_idx)
}

pub(super) fn encryption_tables_for_column(column: &str) -> &'static [&'static str] {
    match column {
        "title" => &["threads", "notifications"],
        "body" => &[
            "threads",
            "comments",
            "conversation_messages",
            "notifications",
        ],
        "payload_json" => &["webhook_jobs"],
        _ => &[
            "threads",
            "comments",
            "conversation_messages",
            "notifications",
            "webhook_jobs",
        ],
    }
}

fn associated_data(table: &str, row_id: &str, column: &str) -> String {
    format!("{table}:{row_id}:{column}")
}

async fn migrate_table_columns(
    mut tx: &mut DbTransaction,
    table: &str,
    columns: &[&str],
) -> anyhow::Result<i64> {
    let select_columns = columns.join(", ");
    let rows = query(&format!("SELECT id, {select_columns} FROM {table}"))
        .fetch_all_unchecked(&mut tx)
        .await?;
    let mut count = 0;
    for row in rows {
        let id: String = row.get("id")?;
        for column in columns {
            let value: String = row.get(column)?;
            if value.starts_with(ENVELOPE_PREFIX) {
                continue;
            }
            query(&format!("UPDATE {table} SET {column} = ? WHERE id = ?"))
                .bind(value)
                .bind(&id)
                .execute_unchecked(&mut tx)
                .await?;
            count += 1;
        }
    }
    Ok(count)
}
