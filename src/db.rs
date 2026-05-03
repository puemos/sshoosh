use std::{
    collections::HashMap,
    marker::PhantomData,
    path::{Path, PathBuf},
    sync::{
        Arc, OnceLock,
        atomic::{AtomicBool, AtomicI64, Ordering},
    },
    time::{Duration, Instant},
};

use anyhow::{Context, bail};
use base64::{Engine as _, engine::general_purpose::URL_SAFE_NO_PAD};
use chacha20poly1305::{
    XChaCha20Poly1305, XNonce,
    aead::{Aead, KeyInit, OsRng},
};
use libsql::{Builder, Connection, TransactionBehavior, Value, params_from_iter};
use rand::RngCore;
use secrecy::{ExposeSecret, SecretBox};
use time::{OffsetDateTime, format_description::well_known::Rfc3339};
use tokio::sync::Mutex;
use url::Url;
use zeroize::Zeroizing;

use crate::domain::parse_labels;

const MIGRATION_INITIAL: &str = include_str!("../migrations/20260430000000_initial.sql");
const MIGRATION_PENDING_USERNAME: &str =
    include_str!("../migrations/20260430000001_pending_username.sql");
const MIGRATION_REMOTE_SECURITY: &str =
    include_str!("../migrations/20260430000001_remote_security.sql");
const MIGRATION_SAVED_MESSAGES: &str =
    include_str!("../migrations/20260501000000_saved_messages.sql");
const MIGRATION_NOTIFICATION_ARCHIVE: &str =
    include_str!("../migrations/20260501000001_notification_archive.sql");
const MIGRATION_PERFORMANCE_COUNTERS: &str =
    include_str!("../migrations/20260501000002_performance_counters.sql");
const MIGRATION_DM_SIDEBAR_SCALE: &str =
    include_str!("../migrations/20260501000003_dm_sidebar_scale.sql");
const MIGRATION_DEVICE_LINK_TOKENS: &str =
    include_str!("../migrations/20260501000004_device_link_tokens.sql");
const MIGRATION_MESSAGE_LABELS: &str =
    include_str!("../migrations/20260501000005_message_labels.sql");
const ENVELOPE_PREFIX: &str = "sshoosh:v1:xchacha20poly1305:";

#[derive(Clone, Debug)]
pub struct DatabaseConfig {
    pub db_path: PathBuf,
    pub database_url: Option<String>,
    pub database_auth_token: Option<SecretBox<str>>,
    pub node_id: String,
    pub encryption_key: Option<SecretBox<str>>,
    pub master_lease_ttl: Duration,
    pub master_heartbeat: Duration,
    pub allow_plaintext_encryption_migration: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DatabaseKind {
    Local,
    Remote,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DbRole {
    Master,
    Standby,
}

impl DbRole {
    fn allow_standby_writes(self) -> bool {
        matches!(self, Self::Master)
    }
}

#[derive(Clone)]
pub struct Database {
    inner: Arc<libsql::Database>,
    kind: DatabaseKind,
    display_name: String,
    encryption: Option<Arc<EncryptionService>>,
    node_id: Arc<str>,
    master_lease_ttl: Duration,
    master_heartbeat: Duration,
    allow_plaintext_encryption_migration: bool,
    is_master: Arc<AtomicBool>,
    fencing_token: Arc<AtomicI64>,
    write_lock: Arc<Mutex<()>>,
    ignore_check_constraints: Arc<AtomicBool>,
    local_path: Option<PathBuf>,
}

pub struct DbTransaction {
    tx: libsql::Transaction,
    encryption: Option<Arc<EncryptionService>>,
    bypass_master_check: bool,
    is_master: Arc<AtomicBool>,
}

pub struct DbReadSession {
    conn: Connection,
    encryption: Option<Arc<EncryptionService>>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DbResult {
    rows_affected: u64,
    last_insert_rowid: i64,
}

impl DbResult {
    pub fn rows_affected(&self) -> u64 {
        self.rows_affected
    }

    pub fn last_insert_rowid(&self) -> i64 {
        self.last_insert_rowid
    }
}

#[derive(Clone)]
pub struct DbRow {
    values: Vec<Value>,
    names: Vec<String>,
    columns: HashMap<String, usize>,
    row_id_hint: Option<String>,
    encryption: Option<Arc<EncryptionService>>,
}

pub trait FromDbValue: Sized {
    fn from_db_value(value: Value) -> anyhow::Result<Self>;
}

pub trait IntoDbValue {
    fn into_db_value(self) -> Value;
}

#[derive(Clone, Debug)]
pub struct Query {
    sql: String,
    params: Vec<Value>,
    bypass_master_check: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
#[allow(dead_code)]
enum QueryMutation {
    Insert { table: String },
    Update { table: String },
    Delete { table: String },
    Replace { table: String },
    Create { target: String },
    Alter { target: String },
    Drop { target: String },
    Vacuum,
    Truncate { target: String },
    Attach,
    Detach,
    Pragma,
    ReadOnly,
}

pub struct QueryScalar<T> {
    inner: Query,
    _marker: PhantomData<T>,
}

pub struct QueryAs<T> {
    inner: Query,
    _marker: PhantomData<T>,
}

pub trait FromDbRow: Sized {
    fn from_db_row(row: DbRow) -> anyhow::Result<Self>;
}

pub fn query(sql: &str) -> Query {
    Query {
        sql: sql.to_string(),
        params: Vec::new(),
        bypass_master_check: false,
    }
}

pub fn query_scalar<T>(sql: &str) -> QueryScalar<T> {
    QueryScalar {
        inner: query(sql),
        _marker: PhantomData,
    }
}

pub fn query_as<T>(sql: &str) -> QueryAs<T> {
    QueryAs {
        inner: query(sql),
        _marker: PhantomData,
    }
}

impl Database {
    pub async fn connect(path: &Path) -> anyhow::Result<Self> {
        let cfg = DatabaseConfig {
            db_path: path.to_path_buf(),
            database_url: None,
            database_auth_token: None,
            node_id: default_node_id(),
            encryption_key: None,
            master_lease_ttl: Duration::from_secs(15),
            master_heartbeat: Duration::from_secs(5),
            allow_plaintext_encryption_migration: false,
        };
        Self::connect_with_config(&cfg).await
    }

    pub async fn connect_with_config(config: &DatabaseConfig) -> anyhow::Result<Self> {
        let (inner, kind, display_name, local_path) = if let Some(url) =
            config.database_url.as_deref()
        {
            validate_database_url(url)?;
            let token = config
                .database_auth_token
                .as_ref()
                .map(|token| token.expose_secret().to_string())
                .unwrap_or_default();
            let parsed = Url::parse(url).with_context(|| format!("invalid database URL {url}"))?;
            if parsed.scheme() != "file" && token.is_empty() {
                bail!("SSHOOSH_DATABASE_AUTH_TOKEN is required for remote database URLs");
            }
            let (db, local_path) = if parsed.scheme() == "file" {
                let path = parsed
                    .to_file_path()
                    .map_err(|_| anyhow::anyhow!("invalid file database URL {url}"))?;
                ensure_parent(&path)?;
                let db = Builder::new_local(&path).build().await?;
                let local_path = path.clone();
                secure_local_database_files(&local_path)?;
                (db, Some(local_path))
            } else {
                (
                    Builder::new_remote(url.to_string(), token).build().await?,
                    None,
                )
            };
            let kind = if is_file_url(url) {
                DatabaseKind::Local
            } else {
                DatabaseKind::Remote
            };
            (db, kind, redact_database_url(url), local_path)
        } else {
            ensure_parent(&config.db_path)?;
            let inner = Builder::new_local(&config.db_path).build().await?;
            secure_local_database_files(&config.db_path)?;
            (
                inner,
                DatabaseKind::Local,
                config.db_path.display().to_string(),
                Some(config.db_path.clone()),
            )
        };

        let encryption = config
            .encryption_key
            .as_ref()
            .map(|key| EncryptionService::from_base64url(key.expose_secret()))
            .transpose()?
            .map(Arc::new);

        let db = Self {
            inner: Arc::new(inner),
            kind,
            display_name,
            encryption,
            node_id: Arc::from(config.node_id.as_str()),
            master_lease_ttl: config.master_lease_ttl,
            master_heartbeat: config.master_heartbeat,
            allow_plaintext_encryption_migration: config.allow_plaintext_encryption_migration,
            is_master: Arc::new(AtomicBool::new(true)),
            fencing_token: Arc::new(AtomicI64::new(0)),
            write_lock: Arc::new(Mutex::new(())),
            ignore_check_constraints: Arc::new(AtomicBool::new(false)),
            local_path,
        };

        db.configure_connection(&db.connection()?).await?;
        db.validate_encryption(config.allow_plaintext_encryption_migration)
            .await?;
        Ok(db)
    }

    pub fn read_pool(&self) -> &Self {
        self
    }

    pub fn write_pool(&self) -> &Self {
        self
    }

    pub fn kind(&self) -> DatabaseKind {
        self.kind
    }

    pub fn display_name(&self) -> &str {
        &self.display_name
    }

    pub fn node_id(&self) -> &str {
        &self.node_id
    }

    pub fn is_master(&self) -> bool {
        self.is_master.load(Ordering::Acquire)
    }

    pub fn role(&self) -> DbRole {
        if self.is_master() {
            DbRole::Master
        } else {
            DbRole::Standby
        }
    }

    pub fn set_master_status(&self, is_master: bool, fencing_token: i64) {
        self.is_master.store(is_master, Ordering::Release);
        self.fencing_token.store(fencing_token, Ordering::Release);
    }

    pub fn master_heartbeat(&self) -> Duration {
        self.master_heartbeat
    }

    pub fn master_lease_ttl(&self) -> Duration {
        self.master_lease_ttl
    }

    pub fn encryption_enabled(&self) -> bool {
        self.encryption.is_some()
    }

    pub async fn init(&self) -> anyhow::Result<()> {
        let _guard = self.write_lock.lock().await;
        self.execute_batch_unchecked(
            "CREATE TABLE IF NOT EXISTS _sshoosh_migrations (
               version TEXT PRIMARY KEY,
               applied_at TEXT NOT NULL
             );",
        )
        .await?;
        for (version, sql) in [
            ("20260430000000_initial", MIGRATION_INITIAL),
            (
                "20260430000001_pending_username",
                MIGRATION_PENDING_USERNAME,
            ),
            ("20260430000001_remote_security", MIGRATION_REMOTE_SECURITY),
            ("20260501000000_saved_messages", MIGRATION_SAVED_MESSAGES),
            (
                "20260501000001_notification_archive",
                MIGRATION_NOTIFICATION_ARCHIVE,
            ),
            (
                "20260501000002_performance_counters",
                MIGRATION_PERFORMANCE_COUNTERS,
            ),
            (
                "20260501000003_dm_sidebar_scale",
                MIGRATION_DM_SIDEBAR_SCALE,
            ),
            (
                "20260501000004_device_link_tokens",
                MIGRATION_DEVICE_LINK_TOKENS,
            ),
            ("20260501000005_message_labels", MIGRATION_MESSAGE_LABELS),
        ] {
            let exists: Option<String> =
                query_scalar("SELECT version FROM _sshoosh_migrations WHERE version = ?")
                    .bind(version)
                    .fetch_optional_unchecked(self)
                    .await?;
            if exists.is_some() {
                continue;
            }
            if version == "20260501000001_notification_archive"
                && self.notification_archive_column_exists().await?
            {
                self.execute_batch_unchecked(
                    "CREATE INDEX IF NOT EXISTS idx_notifications_account_archived
                       ON notifications(account_id, archived_at, created_at DESC);",
                )
                .await?;
                query("INSERT INTO _sshoosh_migrations (version, applied_at) VALUES (?, ?)")
                    .bind(version)
                    .bind(now())
                    .execute_unchecked(self)
                    .await?;
                continue;
            }
            if version == "20260501000002_performance_counters"
                && self.performance_counter_columns_exist().await?
            {
                query("INSERT INTO _sshoosh_migrations (version, applied_at) VALUES (?, ?)")
                    .bind(version)
                    .bind(now())
                    .execute_unchecked(self)
                    .await?;
                continue;
            }
            self.execute_batch_unchecked(sql).await?;
            query("INSERT INTO _sshoosh_migrations (version, applied_at) VALUES (?, ?)")
                .bind(version)
                .bind(now())
                .execute_unchecked(self)
                .await?;
        }
        self.backfill_message_labels_once().await?;
        self.validate_encryption(self.allow_plaintext_encryption_migration)
            .await?;
        Ok(())
    }

    async fn backfill_message_labels_once(&self) -> anyhow::Result<()> {
        let version = "20260501000005_message_labels_backfill";
        let exists: Option<String> =
            query_scalar("SELECT version FROM _sshoosh_migrations WHERE version = ?")
                .bind(version)
                .fetch_optional_unchecked(self)
                .await?;
        if exists.is_some() {
            return Ok(());
        }
        self.backfill_message_labels().await?;
        query("INSERT INTO _sshoosh_migrations (version, applied_at) VALUES (?, ?)")
            .bind(version)
            .bind(now())
            .execute_unchecked(self)
            .await?;
        Ok(())
    }

    async fn backfill_message_labels(&self) -> anyhow::Result<()> {
        let mut tx = self.transaction_unchecked().await?;
        let rows = query(
            "SELECT 'thread' AS source_kind,
                    t.id AS source_id,
                    t.channel_id,
                    t.id AS thread_id,
                    NULL AS conversation_id,
                    NULL AS obj_index,
                    t.title,
                    t.body,
                    t.created_at
             FROM threads t
             WHERE t.deleted_at IS NULL
             UNION ALL
             SELECT 'comment' AS source_kind,
                    cm.id AS source_id,
                    cm.channel_id,
                    cm.thread_id,
                    NULL AS conversation_id,
                    cm.obj_index,
                    '' AS title,
                    cm.body,
                    cm.created_at
             FROM comments cm
             JOIN threads t ON t.id = cm.thread_id
             WHERE cm.deleted_at IS NULL AND t.deleted_at IS NULL
             UNION ALL
             SELECT 'dm' AS source_kind,
                    dm.id AS source_id,
                    NULL AS channel_id,
                    NULL AS thread_id,
                    dm.conversation_id,
                    dm.obj_index,
                    '' AS title,
                    dm.body,
                    dm.created_at
             FROM conversation_messages dm
             WHERE dm.deleted_at IS NULL",
        )
        .fetch_all_unchecked(&mut tx)
        .await?;
        for row in rows {
            let source_kind: String = row.get("source_kind")?;
            let source_id: String = row.get("source_id")?;
            let channel_id: Option<String> = row.get("channel_id")?;
            let thread_id: Option<String> = row.get("thread_id")?;
            let conversation_id: Option<String> = row.get("conversation_id")?;
            let obj_index: Option<i64> = row.get("obj_index")?;
            let title: String = row.get("title")?;
            let body: String = row.get("body")?;
            let created_at: String = row.get("created_at")?;
            let text = if title.is_empty() {
                body
            } else {
                format!("{title}\n{body}")
            };
            for tag in parse_labels(&text) {
                query(
                    "INSERT OR IGNORE INTO message_labels
                     (tag, source_kind, source_id, channel_id, thread_id, conversation_id, obj_index, created_at)
                     VALUES (?, ?, ?, ?, ?, ?, ?, ?)",
                )
                .bind(tag)
                .bind(&source_kind)
                .bind(&source_id)
                .bind(channel_id.as_deref())
                .bind(thread_id.as_deref())
                .bind(conversation_id.as_deref())
                .bind(obj_index)
                .bind(&created_at)
                .execute_unchecked(&mut tx)
                .await?;
            }
        }
        tx.commit().await?;
        Ok(())
    }

    async fn notification_archive_column_exists(&self) -> anyhow::Result<bool> {
        let count: i64 = query_scalar(
            "SELECT COUNT(*)
             FROM pragma_table_info('notifications')
             WHERE name = 'archived_at'",
        )
        .fetch_one_unchecked(self)
        .await?;
        Ok(count > 0)
    }

    async fn performance_counter_columns_exist(&self) -> anyhow::Result<bool> {
        let thread_count: i64 = query_scalar(
            "SELECT COUNT(*)
             FROM pragma_table_info('thread_reads')
             WHERE name = 'unread_count'",
        )
        .fetch_one_unchecked(self)
        .await?;
        let conversation_count: i64 = query_scalar(
            "SELECT COUNT(*)
             FROM pragma_table_info('conversation_members')
             WHERE name = 'unread_count'",
        )
        .fetch_one_unchecked(self)
        .await?;
        Ok(thread_count > 0 && conversation_count > 0)
    }

    pub async fn doctor(&self) -> anyhow::Result<DoctorReport> {
        query_scalar::<i64>("SELECT 1")
            .fetch_one_unchecked(self)
            .await?;
        let migration_count: i64 = query_scalar("SELECT COUNT(*) FROM _sshoosh_migrations")
            .fetch_one_unchecked(self)
            .await
            .unwrap_or(0);
        let lease = self.master_status().await.ok().flatten();
        if self.kind == DatabaseKind::Local {
            let result: String = query_scalar("PRAGMA integrity_check")
                .fetch_one_unchecked(self)
                .await?;
            anyhow::ensure!(result == "ok", "sqlite integrity_check failed: {result}");
        }
        Ok(DoctorReport {
            kind: self.kind,
            display_name: self.display_name.clone(),
            migration_count,
            encryption_enabled: self.encryption_enabled(),
            lease,
        })
    }

    pub async fn repair_search_index(&self) -> anyhow::Result<()> {
        let mut tx = self.transaction().await?;
        query("DELETE FROM search_index").execute(&mut tx).await?;
        query(
            "INSERT INTO search_index
             (kind, object_id, channel_id, thread_id, conversation_id, title, body, context)
             SELECT 'thread', t.id, t.channel_id, t.id, NULL, t.title, t.body, '#' || c.slug
             FROM threads t
             JOIN channels c ON c.id = t.channel_id
             WHERE t.deleted_at IS NULL",
        )
        .execute(&mut tx)
        .await?;
        query(
            "INSERT INTO search_index
             (kind, object_id, channel_id, thread_id, conversation_id, title, body, context)
             SELECT 'comment', cm.id, cm.channel_id, cm.thread_id, NULL, t.title, cm.body, '#' || c.slug
             FROM comments cm
             JOIN threads t ON t.id = cm.thread_id
             JOIN channels c ON c.id = cm.channel_id
             WHERE cm.deleted_at IS NULL AND t.deleted_at IS NULL",
        )
        .execute(&mut tx)
        .await?;
        query(
            "INSERT INTO search_index
             (kind, object_id, channel_id, thread_id, conversation_id, title, body, context)
             SELECT 'dm', m.id, NULL, NULL, m.conversation_id, 'DM', m.body, 'DM'
             FROM conversation_messages m
             WHERE m.deleted_at IS NULL",
        )
        .execute(&mut tx)
        .await?;
        tx.commit().await?;
        Ok(())
    }

    pub async fn backup_to(&self, out: &str) -> anyhow::Result<()> {
        if self.kind == DatabaseKind::Remote {
            bail!("remote libSQL/Turso backup is not supported by sshoosh yet");
        }
        let path = Path::new(out);
        anyhow::ensure!(
            !path.exists(),
            "backup output already exists; refusing to overwrite {out}"
        );
        let escaped = out.replace('\'', "''");
        let _umask = RestrictiveUmask::new();
        self.execute_batch_unchecked(&format!("VACUUM INTO '{escaped}'"))
            .await?;
        secure_local_database_files(path)?;
        Ok(())
    }

    pub async fn transaction(&self) -> anyhow::Result<DbTransaction> {
        let conn = self.connection()?;
        self.configure_connection(&conn).await?;
        let tx = conn
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .await?;
        Ok(DbTransaction {
            tx,
            encryption: self.encryption.clone(),
            bypass_master_check: false,
            is_master: self.is_master.clone(),
        })
    }

    pub async fn transaction_unchecked(&self) -> anyhow::Result<DbTransaction> {
        let mut tx = self.transaction().await?;
        tx.bypass_master_check = true;
        Ok(tx)
    }

    pub async fn begin(&self) -> anyhow::Result<DbTransaction> {
        self.transaction().await
    }

    pub async fn read_session(&self) -> anyhow::Result<DbReadSession> {
        let conn = self.connection()?;
        self.configure_connection(&conn).await?;
        Ok(DbReadSession {
            conn,
            encryption: self.encryption.clone(),
        })
    }

    pub async fn master_status(&self) -> anyhow::Result<Option<MasterStatus>> {
        query(
            "SELECT node_id, fencing_token, lease_until, heartbeat_at
             FROM server_leases
             WHERE name = 'main'",
        )
        .fetch_optional_unchecked(self)
        .await?
        .map(|row| {
            let node_id: String = row.get("node_id")?;
            Ok::<MasterStatus, anyhow::Error>(MasterStatus {
                node_id: node_id.clone(),
                fencing_token: row.get("fencing_token")?,
                lease_until: row.get("lease_until")?,
                heartbeat_at: row.get("heartbeat_at")?,
                is_this_node: node_id == self.node_id(),
            })
        })
        .transpose()
    }

    pub async fn try_acquire_or_renew_master(&self) -> anyhow::Result<bool> {
        let now = now();
        let lease_until = format_rfc3339(OffsetDateTime::now_utc() + self.master_lease_ttl);
        let mut tx = self.transaction_unchecked().await?;
        query(
            "INSERT INTO server_leases (name, node_id, fencing_token, lease_until, heartbeat_at)
             VALUES ('main', ?, 1, ?, ?)
             ON CONFLICT(name) DO NOTHING",
        )
        .bind(self.node_id())
        .bind(&lease_until)
        .bind(&now)
        .execute_unchecked(&mut tx)
        .await?;

        let row = query(
            "SELECT node_id, fencing_token, lease_until
             FROM server_leases
             WHERE name = 'main'",
        )
        .fetch_one_unchecked(&mut tx)
        .await?;
        let current_node: String = row.get("node_id")?;
        let current_token: i64 = row.get("fencing_token")?;
        let current_until: String = row.get("lease_until")?;
        let expired = parse_rfc3339(&current_until)
            .map(|until| until < OffsetDateTime::now_utc())
            .unwrap_or(true);

        let acquired = if current_node == self.node_id() {
            let changed = query(
                "UPDATE server_leases
                 SET lease_until = ?, heartbeat_at = ?
                 WHERE name = 'main' AND node_id = ? AND fencing_token = ?",
            )
            .bind(&lease_until)
            .bind(&now)
            .bind(self.node_id())
            .bind(current_token)
            .execute_unchecked(&mut tx)
            .await?
            .rows_affected()
                > 0;
            if changed {
                self.set_master_status(true, current_token);
            }
            changed
        } else if expired {
            let next_token = current_token + 1;
            let changed = query(
                "UPDATE server_leases
                 SET node_id = ?, fencing_token = ?, lease_until = ?, heartbeat_at = ?
                 WHERE name = 'main' AND node_id = ? AND fencing_token = ? AND lease_until = ?",
            )
            .bind(self.node_id())
            .bind(next_token)
            .bind(&lease_until)
            .bind(&now)
            .bind(&current_node)
            .bind(current_token)
            .bind(&current_until)
            .execute_unchecked(&mut tx)
            .await?
            .rows_affected()
                > 0;
            if changed {
                self.set_master_status(true, next_token);
            }
            changed
        } else {
            self.set_master_status(false, current_token);
            false
        };
        tx.commit().await?;
        Ok(acquired)
    }

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

    fn connection(&self) -> anyhow::Result<Connection> {
        self.inner.connect().map_err(Into::into)
    }

    async fn configure_connection(&self, conn: &Connection) -> anyhow::Result<()> {
        if self.kind == DatabaseKind::Local {
            let _ = conn.busy_timeout(Duration::from_secs(5));
            conn.execute("PRAGMA foreign_keys = ON", ()).await?;
            conn.execute("PRAGMA temp_store = MEMORY", ()).await?;
            conn.execute("PRAGMA journal_mode = WAL", ()).await.ok();
            conn.execute("PRAGMA synchronous = NORMAL", ()).await.ok();
            if let Some(path) = self.local_path.as_deref() {
                secure_local_database_files(path)?;
            }
            if self.ignore_check_constraints.load(Ordering::Acquire) {
                conn.execute("PRAGMA ignore_check_constraints = ON", ())
                    .await
                    .ok();
            }
        }
        Ok(())
    }

    async fn execute_batch_unchecked(&self, sql: &str) -> anyhow::Result<()> {
        let conn = self.connection()?;
        self.configure_connection(&conn).await?;
        for statement in sql
            .split(';')
            .map(str::trim)
            .filter(|stmt| !stmt.is_empty())
        {
            conn.execute(statement, ()).await.with_context(|| {
                format!("executing migration SQL: {}", summarize_sql(statement))
            })?;
        }
        Ok(())
    }

    async fn validate_encryption(&self, allow_plaintext: bool) -> anyhow::Result<()> {
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

impl DbTransaction {
    pub async fn commit(self) -> anyhow::Result<()> {
        self.tx.commit().await?;
        Ok(())
    }

    pub async fn rollback(self) -> anyhow::Result<()> {
        self.tx.rollback().await?;
        Ok(())
    }
}

#[allow(async_fn_in_trait)]
pub trait DbExecutor {
    async fn execute_query(&mut self, query: Query) -> anyhow::Result<DbResult>;
    async fn fetch_rows(&mut self, query: Query) -> anyhow::Result<Vec<DbRow>>;
}

fn require_master_for_write(
    role: DbRole,
    allow_standby_writes: bool,
    query: &Query,
) -> anyhow::Result<()> {
    if query.bypass_master_check {
        return Ok(());
    }
    if !allow_standby_writes && !role.allow_standby_writes() && query.is_write() {
        bail!("master lease required for write query");
    }
    Ok(())
}

impl DbExecutor for &Database {
    async fn execute_query(&mut self, mut query: Query) -> anyhow::Result<DbResult> {
        if normalize_sql(&query.sql).starts_with("pragma ignore_check_constraints = on") {
            self.ignore_check_constraints.store(true, Ordering::Release);
        }
        require_master_for_write(self.role(), false, &query)?;
        query.encrypt_params(self.encryption.as_deref())?;
        let conn = self.connection()?;
        self.configure_connection(&conn).await?;
        let started = Instant::now();
        let rows_affected = conn
            .execute(&query.sql, params_from_iter(query.params))
            .await
            .with_context(|| format!("executing SQL: {}", summarize_sql(&query.sql)))?;
        trace_query(
            "execute",
            &query.sql,
            started.elapsed(),
            Some(rows_affected),
            None,
        );
        let mut rows = conn.query("SELECT last_insert_rowid()", ()).await?;
        let last_insert_rowid = rows
            .next()
            .await?
            .and_then(|row| row.get::<i64>(0).ok())
            .unwrap_or(0);
        Ok(DbResult {
            rows_affected,
            last_insert_rowid,
        })
    }

    async fn fetch_rows(&mut self, query: Query) -> anyhow::Result<Vec<DbRow>> {
        require_master_for_write(self.role(), false, &query)?;
        let conn = self.connection()?;
        self.configure_connection(&conn).await?;
        let row_id_hint = query.row_id_hint();
        let sql = query.sql.clone();
        let started = Instant::now();
        let mut rows = conn
            .query(&query.sql, params_from_iter(query.params))
            .await
            .with_context(|| format!("querying SQL: {}", summarize_sql(&query.sql)))?;
        let rows = collect_rows(&mut rows, self.encryption.clone(), row_id_hint).await?;
        trace_query("query", &sql, started.elapsed(), None, Some(rows.len()));
        Ok(rows)
    }
}

impl DbExecutor for &DbReadSession {
    async fn execute_query(&mut self, mut query: Query) -> anyhow::Result<DbResult> {
        require_master_for_write(DbRole::Standby, false, &query)?;
        query.encrypt_params(self.encryption.as_deref())?;
        let started = Instant::now();
        let rows_affected = self
            .conn
            .execute(&query.sql, params_from_iter(query.params))
            .await
            .with_context(|| format!("executing SQL: {}", summarize_sql(&query.sql)))?;
        trace_query(
            "execute",
            &query.sql,
            started.elapsed(),
            Some(rows_affected),
            None,
        );
        let mut rows = self.conn.query("SELECT last_insert_rowid()", ()).await?;
        let last_insert_rowid = rows
            .next()
            .await?
            .and_then(|row| row.get::<i64>(0).ok())
            .unwrap_or(0);
        Ok(DbResult {
            rows_affected,
            last_insert_rowid,
        })
    }

    async fn fetch_rows(&mut self, query: Query) -> anyhow::Result<Vec<DbRow>> {
        require_master_for_write(DbRole::Standby, false, &query)?;
        let row_id_hint = query.row_id_hint();
        let sql = query.sql.clone();
        let started = Instant::now();
        let mut rows = self
            .conn
            .query(&query.sql, params_from_iter(query.params))
            .await
            .with_context(|| format!("querying SQL: {}", summarize_sql(&query.sql)))?;
        let rows = collect_rows(&mut rows, self.encryption.clone(), row_id_hint).await?;
        trace_query("query", &sql, started.elapsed(), None, Some(rows.len()));
        Ok(rows)
    }
}

impl DbExecutor for &mut DbTransaction {
    async fn execute_query(&mut self, mut query: Query) -> anyhow::Result<DbResult> {
        if self.bypass_master_check {
            query.bypass_master_check = true;
        }
        require_master_for_write(self_role(self), self.bypass_master_check, &query)?;
        query.encrypt_params(self.encryption.as_deref())?;
        let started = Instant::now();
        let rows_affected = self
            .tx
            .execute(&query.sql, params_from_iter(query.params))
            .await
            .with_context(|| format!("executing SQL: {}", summarize_sql(&query.sql)))?;
        trace_query(
            "execute",
            &query.sql,
            started.elapsed(),
            Some(rows_affected),
            None,
        );
        let mut rows = self.tx.query("SELECT last_insert_rowid()", ()).await?;
        let last_insert_rowid = rows
            .next()
            .await?
            .and_then(|row| row.get::<i64>(0).ok())
            .unwrap_or(0);
        Ok(DbResult {
            rows_affected,
            last_insert_rowid,
        })
    }

    async fn fetch_rows(&mut self, mut query: Query) -> anyhow::Result<Vec<DbRow>> {
        if self.bypass_master_check {
            query.bypass_master_check = true;
        }
        require_master_for_write(self_role(self), self.bypass_master_check, &query)?;
        let row_id_hint = query.row_id_hint();
        let sql = query.sql.clone();
        let started = Instant::now();
        let mut rows = self
            .tx
            .query(&query.sql, params_from_iter(query.params))
            .await
            .with_context(|| format!("querying SQL: {}", summarize_sql(&query.sql)))?;
        let rows = collect_rows(&mut rows, self.encryption.clone(), row_id_hint).await?;
        trace_query("query", &sql, started.elapsed(), None, Some(rows.len()));
        Ok(rows)
    }
}

fn self_role(exec: &mut DbTransaction) -> DbRole {
    if exec.is_master.load(Ordering::Acquire) {
        DbRole::Master
    } else {
        DbRole::Standby
    }
}

impl DbExecutor for &mut &mut DbTransaction {
    async fn execute_query(&mut self, query: Query) -> anyhow::Result<DbResult> {
        DbExecutor::execute_query(&mut **self, query).await
    }

    async fn fetch_rows(&mut self, query: Query) -> anyhow::Result<Vec<DbRow>> {
        DbExecutor::fetch_rows(&mut **self, query).await
    }
}

impl Query {
    fn is_write(&self) -> bool {
        query_is_write(&self.sql)
    }

    pub fn bind(mut self, value: impl IntoDbValue) -> Self {
        self.params.push(value.into_db_value());
        self
    }

    pub fn unchecked(mut self) -> Self {
        self.bypass_master_check = true;
        self
    }

    pub async fn execute<E: DbExecutor>(self, mut exec: E) -> anyhow::Result<DbResult> {
        exec.execute_query(self).await
    }

    pub async fn execute_unchecked<E: DbExecutor>(self, mut exec: E) -> anyhow::Result<DbResult> {
        exec.execute_query(self.unchecked()).await
    }

    pub async fn fetch_all<E: DbExecutor>(self, mut exec: E) -> anyhow::Result<Vec<DbRow>> {
        exec.fetch_rows(self).await
    }

    pub async fn fetch_all_unchecked<E: DbExecutor>(
        self,
        mut exec: E,
    ) -> anyhow::Result<Vec<DbRow>> {
        exec.fetch_rows(self.unchecked()).await
    }

    pub async fn fetch_optional<E: DbExecutor>(self, mut exec: E) -> anyhow::Result<Option<DbRow>> {
        let mut rows = exec.fetch_rows(self).await?;
        Ok(rows.pop())
    }

    pub async fn fetch_optional_unchecked<E: DbExecutor>(
        self,
        mut exec: E,
    ) -> anyhow::Result<Option<DbRow>> {
        let mut rows = exec.fetch_rows(self.unchecked()).await?;
        Ok(rows.pop())
    }

    pub async fn fetch_one<E: DbExecutor>(self, exec: E) -> anyhow::Result<DbRow> {
        self.fetch_optional(exec)
            .await?
            .context("query returned no rows")
    }

    pub async fn fetch_one_unchecked<E: DbExecutor>(self, exec: E) -> anyhow::Result<DbRow> {
        self.fetch_optional_unchecked(exec)
            .await?
            .context("query returned no rows")
    }

    fn encrypt_params(&mut self, encryption: Option<&EncryptionService>) -> anyhow::Result<()> {
        let Some(encryption) = encryption else {
            return Ok(());
        };
        let mutation = query_mutation(&self.sql);
        match &mutation {
            Some(QueryMutation::Insert { table }) if table == "threads" => {
                encrypt_param(encryption, &mut self.params, 3, "threads", 0, "title")?;
                encrypt_param(encryption, &mut self.params, 4, "threads", 0, "body")?;
            }
            Some(QueryMutation::Update { table }) if table == "threads" => {
                encrypt_named_update_column(
                    encryption,
                    &mut self.params,
                    &self.sql,
                    "threads",
                    "title",
                )?;
                encrypt_named_update_column(
                    encryption,
                    &mut self.params,
                    &self.sql,
                    "threads",
                    "body",
                )?;
            }
            Some(QueryMutation::Insert { table }) if table == "comments" => {
                encrypt_param(encryption, &mut self.params, 5, "comments", 0, "body")?;
            }
            Some(QueryMutation::Update { table }) if table == "comments" => {
                encrypt_named_update_column(
                    encryption,
                    &mut self.params,
                    &self.sql,
                    "comments",
                    "body",
                )?;
            }
            Some(QueryMutation::Insert { table }) if table == "conversation_messages" => {
                encrypt_param(
                    encryption,
                    &mut self.params,
                    4,
                    "conversation_messages",
                    0,
                    "body",
                )?;
            }
            Some(QueryMutation::Update { table }) if table == "conversation_messages" => {
                encrypt_named_update_column(
                    encryption,
                    &mut self.params,
                    &self.sql,
                    "conversation_messages",
                    "body",
                )?;
            }
            Some(QueryMutation::Insert { table }) if table == "notifications" => {
                encrypt_param(encryption, &mut self.params, 9, "notifications", 0, "title")?;
                encrypt_param(encryption, &mut self.params, 10, "notifications", 0, "body")?;
            }
            Some(QueryMutation::Insert { table }) if table == "webhook_jobs" => {
                encrypt_named_payload(encryption, &mut self.params, &self.sql, &mutation)?;
            }
            Some(QueryMutation::Update { table }) if table == "webhook_jobs" => {
                encrypt_named_update_column(
                    encryption,
                    &mut self.params,
                    &self.sql,
                    "webhook_jobs",
                    "payload_json",
                )?;
            }
            _ => {}
        }
        Ok(())
    }

    fn row_id_hint(&self) -> Option<String> {
        let sql = normalize_sql(&self.sql);
        if sql.contains(" where id = ?") {
            self.params.first().and_then(value_as_str)
        } else {
            None
        }
    }
}

fn query_is_write(sql: &str) -> bool {
    !matches!(
        query_mutation(&normalize_sql(sql)),
        Some(QueryMutation::ReadOnly)
    )
}

fn query_mutation(sql: &str) -> Option<QueryMutation> {
    let normalized = normalize_sql(sql);
    let statement = if normalized.starts_with("with ") {
        strip_with_clause_prefix(&normalized)
    } else {
        normalized.as_str()
    };
    let mut tokens = statement.split_whitespace();
    let token = tokens.next()?;
    match token {
        "insert" => parse_insert(tokens),
        "select" | "explain" => Some(QueryMutation::ReadOnly),
        "or" => match tokens.next()? {
            "replace" => parse_insert(tokens).map(|mutation| match mutation {
                QueryMutation::Insert { table } => QueryMutation::Replace { table },
                _ => mutation,
            }),
            "ignore" | "rollback" | "abort" | "fail" => parse_insert(tokens),
            "update" => parse_update(tokens),
            _ => None,
        },
        "update" => parse_update(tokens),
        "delete" => {
            let token = tokens.next()?;
            if token == "from" {
                tokens.next().map(|table| QueryMutation::Delete {
                    table: table.to_string(),
                })
            } else {
                None
            }
        }
        "replace" => parse_insert(tokens).map(|m| match m {
            QueryMutation::Insert { table } => QueryMutation::Replace { table },
            _ => m,
        }),
        "create" | "alter" | "drop" | "truncate" | "attach" | "detach" => {
            parse_schema_mutation(token, &mut tokens)
        }
        "vacuum" => Some(QueryMutation::Vacuum),
        "begin" | "commit" | "rollback" | "savepoint" | "release" => Some(QueryMutation::ReadOnly),
        "pragma" => {
            if statement.contains(" = ") {
                Some(QueryMutation::Pragma)
            } else {
                Some(QueryMutation::ReadOnly)
            }
        }
        _ => None,
    }
}

fn strip_with_clause_prefix(sql: &str) -> &str {
    let mut depth = 0usize;
    let bytes = sql.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        match bytes[i] as char {
            '(' => depth = depth.saturating_add(1),
            ')' => {
                if depth > 0 {
                    depth = depth.saturating_sub(1);
                }
                if depth == 0 {
                    let mut j = i + 1;
                    while j < bytes.len() && bytes[j].is_ascii_whitespace() {
                        j += 1;
                    }
                    if j < bytes.len() && bytes[j] == b',' {
                        i = j;
                        continue;
                    }
                    if j < bytes.len() {
                        return &sql[j..];
                    }
                }
            }
            _ => {}
        }
        i += 1;
    }
    sql
}

fn parse_insert<'a, I>(mut tokens: I) -> Option<QueryMutation>
where
    I: Iterator<Item = &'a str>,
{
    let mut token = tokens.next()?;
    if token == "or" {
        token = tokens.next()?;
    }
    let table = if token == "into" {
        tokens.next()?
    } else {
        token
    };
    Some(QueryMutation::Insert {
        table: table.to_string(),
    })
}

fn parse_update<'a, I>(mut tokens: I) -> Option<QueryMutation>
where
    I: Iterator<Item = &'a str>,
{
    let table = tokens.next()?;
    Some(QueryMutation::Update {
        table: table.to_string(),
    })
}

fn parse_schema_mutation<'a>(
    token: &str,
    mut tokens: impl Iterator<Item = &'a str>,
) -> Option<QueryMutation> {
    let target = tokens
        .next()
        .map(|target| strip_schema_qualifier(token, target));
    target.map(|target| match token {
        "create" => QueryMutation::Create { target },
        "alter" => QueryMutation::Alter { target },
        "drop" => QueryMutation::Drop { target },
        "truncate" => QueryMutation::Truncate { target },
        "attach" => QueryMutation::Attach,
        "detach" => QueryMutation::Detach,
        _ => QueryMutation::Create { target },
    })
}

fn strip_schema_qualifier(token: &str, value: &str) -> String {
    if token == "attach" || token == "detach" {
        value.to_string()
    } else {
        value.trim_matches(&['"', '`', '\''][..]).to_string()
    }
}

impl<T> QueryScalar<T> {
    pub fn bind(mut self, value: impl IntoDbValue) -> Self {
        self.inner = self.inner.bind(value);
        self
    }

    pub async fn fetch_one<E: DbExecutor>(self, exec: E) -> anyhow::Result<T>
    where
        T: FromDbValue,
    {
        let row = self.inner.fetch_one(exec).await?;
        row.try_get_idx(0)
    }

    pub async fn fetch_one_unchecked<E: DbExecutor>(self, exec: E) -> anyhow::Result<T>
    where
        T: FromDbValue,
    {
        let row = self.inner.fetch_one_unchecked(exec).await?;
        row.try_get_idx(0)
    }

    pub async fn fetch_optional<E: DbExecutor>(self, exec: E) -> anyhow::Result<Option<T>>
    where
        T: FromDbValue,
    {
        self.inner
            .fetch_optional(exec)
            .await?
            .map(|row| row.try_get_idx(0))
            .transpose()
    }

    pub async fn fetch_optional_unchecked<E: DbExecutor>(self, exec: E) -> anyhow::Result<Option<T>>
    where
        T: FromDbValue,
    {
        self.inner
            .fetch_optional_unchecked(exec)
            .await?
            .map(|row| row.try_get_idx(0))
            .transpose()
    }

    pub async fn fetch_all<E: DbExecutor>(self, exec: E) -> anyhow::Result<Vec<T>>
    where
        T: FromDbValue,
    {
        self.inner
            .fetch_all(exec)
            .await?
            .into_iter()
            .map(|row| row.try_get_idx(0))
            .collect()
    }
}

impl<T> QueryAs<T> {
    pub fn bind(mut self, value: impl IntoDbValue) -> Self {
        self.inner = self.inner.bind(value);
        self
    }

    pub async fn fetch_optional<E: DbExecutor>(self, exec: E) -> anyhow::Result<Option<T>>
    where
        T: FromDbRow,
    {
        self.inner
            .fetch_optional(exec)
            .await?
            .map(T::from_db_row)
            .transpose()
    }
}

impl DbRow {
    pub fn get<T: FromDbValue>(&self, name: &str) -> anyhow::Result<T> {
        self.try_get(name)
    }

    pub fn try_get<T: FromDbValue>(&self, name: &str) -> anyhow::Result<T> {
        let idx = self
            .columns
            .get(name)
            .copied()
            .or_else(|| self.columns.get(&name.to_ascii_lowercase()).copied())
            .with_context(|| format!("column not found: {name}"))?;
        self.try_get_idx(idx)
    }

    pub fn get_idx<T: FromDbValue>(&self, idx: usize) -> anyhow::Result<T> {
        self.try_get_idx(idx)
    }

    pub fn try_get_idx<T: FromDbValue>(&self, idx: usize) -> anyhow::Result<T> {
        let value = self
            .values
            .get(idx)
            .cloned()
            .with_context(|| format!("column index out of range: {idx}"))?;
        T::from_db_value(self.decrypt_value(idx, value)?)
    }

    pub fn columns(&self) -> Vec<String> {
        self.names.clone()
    }

    fn decrypt_value(&self, idx: usize, value: Value) -> anyhow::Result<Value> {
        let Value::Text(text) = value else {
            return Ok(value);
        };
        if !text.starts_with(ENVELOPE_PREFIX) {
            return Ok(Value::Text(text));
        }
        let Some(encryption) = self.encryption.as_deref() else {
            bail!("encrypted content exists but SSHOOSH_ENCRYPTION_KEY is not configured");
        };
        let id = self
            .columns
            .get("id")
            .and_then(|id_idx| self.values.get(*id_idx))
            .and_then(value_as_str)
            .or_else(|| self.row_id_hint.clone())
            .unwrap_or_default();
        let column = self
            .columns
            .iter()
            .find_map(|(name, col_idx)| {
                (*col_idx == idx && !name.is_empty()).then_some(name.as_str())
            })
            .unwrap_or("");
        for table in encryption_tables_for_column(column) {
            if let Ok(plain) = encryption.decrypt(table, &id, column, &text) {
                return Ok(Value::Text(plain));
            }
        }
        bail!("failed to decrypt encrypted database content; check SSHOOSH_ENCRYPTION_KEY")
    }
}

impl FromDbValue for String {
    fn from_db_value(value: Value) -> anyhow::Result<Self> {
        match value {
            Value::Text(value) => Ok(value),
            Value::Integer(value) => Ok(value.to_string()),
            Value::Real(value) => Ok(value.to_string()),
            Value::Null => bail!("unexpected NULL string"),
            Value::Blob(_) => bail!("unexpected BLOB string"),
        }
    }
}

impl FromDbValue for Option<String> {
    fn from_db_value(value: Value) -> anyhow::Result<Self> {
        match value {
            Value::Null => Ok(None),
            value => String::from_db_value(value).map(Some),
        }
    }
}

impl FromDbValue for i64 {
    fn from_db_value(value: Value) -> anyhow::Result<Self> {
        match value {
            Value::Integer(value) => Ok(value),
            Value::Text(value) => Ok(value.parse()?),
            Value::Null => bail!("unexpected NULL integer"),
            _ => bail!("unexpected non-integer value"),
        }
    }
}

impl FromDbValue for Option<i64> {
    fn from_db_value(value: Value) -> anyhow::Result<Self> {
        match value {
            Value::Null => Ok(None),
            value => i64::from_db_value(value).map(Some),
        }
    }
}

impl FromDbValue for bool {
    fn from_db_value(value: Value) -> anyhow::Result<Self> {
        Ok(i64::from_db_value(value)? != 0)
    }
}

impl FromDbRow for (Option<String>, Option<String>) {
    fn from_db_row(row: DbRow) -> anyhow::Result<Self> {
        Ok((row.try_get_idx(0)?, row.try_get_idx(1)?))
    }
}

impl IntoDbValue for Value {
    fn into_db_value(self) -> Value {
        self
    }
}

impl IntoDbValue for &str {
    fn into_db_value(self) -> Value {
        Value::Text(self.to_string())
    }
}

impl IntoDbValue for String {
    fn into_db_value(self) -> Value {
        Value::Text(self)
    }
}

impl IntoDbValue for &String {
    fn into_db_value(self) -> Value {
        Value::Text(self.clone())
    }
}

impl IntoDbValue for Option<&str> {
    fn into_db_value(self) -> Value {
        self.map(|value| Value::Text(value.to_string()))
            .unwrap_or(Value::Null)
    }
}

impl IntoDbValue for Option<String> {
    fn into_db_value(self) -> Value {
        self.map(Value::Text).unwrap_or(Value::Null)
    }
}

impl IntoDbValue for Option<i64> {
    fn into_db_value(self) -> Value {
        self.map(Value::Integer).unwrap_or(Value::Null)
    }
}

impl IntoDbValue for Option<&String> {
    fn into_db_value(self) -> Value {
        self.map(|value| Value::Text(value.clone()))
            .unwrap_or(Value::Null)
    }
}

impl IntoDbValue for i64 {
    fn into_db_value(self) -> Value {
        Value::Integer(self)
    }
}

impl IntoDbValue for i32 {
    fn into_db_value(self) -> Value {
        Value::Integer(self as i64)
    }
}

impl IntoDbValue for u64 {
    fn into_db_value(self) -> Value {
        Value::Integer(self as i64)
    }
}

impl IntoDbValue for bool {
    fn into_db_value(self) -> Value {
        Value::Integer(i64::from(self))
    }
}

#[derive(Clone, Debug)]
pub struct MasterStatus {
    pub node_id: String,
    pub fencing_token: i64,
    pub lease_until: String,
    pub heartbeat_at: String,
    pub is_this_node: bool,
}

#[derive(Clone, Debug)]
pub struct DoctorReport {
    pub kind: DatabaseKind,
    pub display_name: String,
    pub migration_count: i64,
    pub encryption_enabled: bool,
    pub lease: Option<MasterStatus>,
}

#[derive(Clone, Debug, Default, serde::Serialize)]
pub struct EncryptionMigrationReport {
    pub threads: i64,
    pub comments: i64,
    pub conversation_messages: i64,
    pub notifications: i64,
}

pub struct EncryptionService {
    cipher: XChaCha20Poly1305,
}

impl EncryptionService {
    fn from_base64url(key: &str) -> anyhow::Result<Self> {
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

    fn decrypt(
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

async fn collect_rows(
    rows: &mut libsql::Rows,
    encryption: Option<Arc<EncryptionService>>,
    row_id_hint: Option<String>,
) -> anyhow::Result<Vec<DbRow>> {
    let mut out = Vec::new();
    while let Some(row) = rows.next().await? {
        let mut values = Vec::new();
        let mut names = Vec::new();
        let mut columns = HashMap::new();
        for idx in 0..row.column_count() {
            let name = row.column_name(idx).unwrap_or("").to_string();
            columns.insert(name.clone(), idx as usize);
            columns.insert(name.to_ascii_lowercase(), idx as usize);
            names.push(name);
            values.push(row.get_value(idx)?);
        }
        out.push(DbRow {
            values,
            names,
            columns,
            row_id_hint: row_id_hint.clone(),
            encryption: encryption.clone(),
        });
    }
    Ok(out)
}

fn encrypt_param(
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

fn encrypt_named_payload(
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

fn encrypt_named_update_column(
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

fn value_as_str(value: &Value) -> Option<String> {
    match value {
        Value::Text(value) => Some(value.clone()),
        Value::Integer(value) => Some(value.to_string()),
        _ => None,
    }
}

fn encryption_tables_for_column(column: &str) -> &'static [&'static str] {
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

pub fn default_node_id() -> String {
    let host = hostname::get()
        .ok()
        .and_then(|name| name.into_string().ok())
        .filter(|name| !name.trim().is_empty())
        .unwrap_or_else(|| "unknown-host".to_string());
    let mut suffix = [0u8; 4];
    rand::thread_rng().fill_bytes(&mut suffix);
    format!(
        "{}-{}-{}",
        host,
        std::process::id(),
        URL_SAFE_NO_PAD.encode(suffix)
    )
}

pub fn now() -> String {
    format_rfc3339(OffsetDateTime::now_utc())
}

pub fn format_rfc3339(value: OffsetDateTime) -> String {
    value.format(&Rfc3339).unwrap_or_else(|_| value.to_string())
}

fn parse_rfc3339(value: &str) -> anyhow::Result<OffsetDateTime> {
    Ok(OffsetDateTime::parse(value, &Rfc3339)?)
}

fn ensure_parent(path: &Path) -> anyhow::Result<()> {
    if let Some(parent) = path.parent().filter(|p| !p.as_os_str().is_empty()) {
        std::fs::create_dir_all(parent)
            .with_context(|| format!("creating {}", parent.display()))?;
    }
    Ok(())
}

fn secure_local_database_files(path: &Path) -> anyhow::Result<()> {
    #[cfg(not(unix))]
    let _ = path;
    #[cfg(unix)]
    {
        secure_local_database_file(path)?;
        secure_local_database_file(&sqlite_sidecar_path(path, "-wal"))?;
        secure_local_database_file(&sqlite_sidecar_path(path, "-shm"))?;
    }
    Ok(())
}

#[cfg(unix)]
struct RestrictiveUmask {
    previous: libc::mode_t,
}

#[cfg(unix)]
impl RestrictiveUmask {
    fn new() -> Self {
        Self {
            // SAFETY: umask is process-global. This guard restores the previous mask on drop.
            previous: unsafe { libc::umask(0o077) },
        }
    }
}

#[cfg(unix)]
impl Drop for RestrictiveUmask {
    fn drop(&mut self) {
        // SAFETY: restoring a previously returned umask value.
        unsafe {
            libc::umask(self.previous);
        }
    }
}

#[cfg(not(unix))]
struct RestrictiveUmask;

#[cfg(not(unix))]
impl RestrictiveUmask {
    fn new() -> Self {
        Self
    }
}

#[cfg(unix)]
fn secure_local_database_file(path: &Path) -> anyhow::Result<()> {
    use std::os::unix::fs::PermissionsExt;

    if path.exists() {
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600))
            .with_context(|| format!("securing permissions for {}", path.display()))?;
    }
    Ok(())
}

#[cfg(unix)]
fn sqlite_sidecar_path(path: &Path, suffix: &str) -> PathBuf {
    let mut value = path.as_os_str().to_os_string();
    value.push(suffix);
    PathBuf::from(value)
}

fn validate_database_url(url: &str) -> anyhow::Result<()> {
    let parsed = Url::parse(url).with_context(|| format!("invalid database URL {url}"))?;
    anyhow::ensure!(
        is_file_url(url) || is_remote_url(url),
        "unsupported database URL scheme '{}'",
        parsed.scheme()
    );
    if is_http_url(url) && !is_local_http_database_url(url) {
        bail!("plain HTTP database URLs are only allowed for localhost development");
    }
    Ok(())
}

fn is_local_http_database_url(url: &str) -> bool {
    let parsed = match Url::parse(url) {
        Ok(parsed) if parsed.scheme() == "http" => parsed,
        _ => return false,
    };
    let Some(host) = parsed.host() else {
        return false;
    };
    match host {
        url::Host::Domain(host) => matches!(host.to_ascii_lowercase().as_str(), "localhost"),
        url::Host::Ipv4(ipv4) => ipv4.is_loopback(),
        url::Host::Ipv6(ipv6) => ipv6.is_loopback(),
    }
}

fn is_remote_url(url: &str) -> bool {
    let Some(parsed) = Url::parse(url).ok() else {
        return false;
    };
    matches!(parsed.scheme(), "http" | "https" | "libsql")
}

fn is_file_url(url: &str) -> bool {
    Url::parse(url)
        .ok()
        .is_some_and(|parsed| parsed.scheme() == "file")
}

fn is_http_url(url: &str) -> bool {
    Url::parse(url)
        .ok()
        .is_some_and(|parsed| parsed.scheme() == "http")
}

fn redact_database_url(url: &str) -> String {
    if let Some((scheme, rest)) = url.split_once("://")
        && let Some((_, host)) = rest.rsplit_once('@')
    {
        return format!("{scheme}://<redacted>@{host}");
    }
    url.to_string()
}

fn summarize_sql(sql: &str) -> String {
    normalize_sql(sql).chars().take(160).collect()
}

fn trace_query(
    operation: &'static str,
    sql: &str,
    elapsed: Duration,
    rows_affected: Option<u64>,
    row_count: Option<usize>,
) {
    static SLOW_QUERY_MS: OnceLock<u128> = OnceLock::new();
    let slow_ms = *SLOW_QUERY_MS.get_or_init(|| {
        std::env::var("SSHOOSH_SLOW_QUERY_MS")
            .ok()
            .and_then(|value| value.parse::<u128>().ok())
            .unwrap_or(50)
    });
    if elapsed.as_millis() >= slow_ms {
        tracing::warn!(
            operation,
            elapsed_ms = elapsed.as_millis() as u64,
            rows_affected,
            row_count,
            sql = %summarize_sql(sql),
            "slow database query"
        );
    } else {
        tracing::trace!(
            operation,
            elapsed_ms = elapsed.as_millis() as u64,
            rows_affected,
            row_count,
            sql = %summarize_sql(sql),
            "database query"
        );
    }
}

fn normalize_sql(sql: &str) -> String {
    sql.split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .to_ascii_lowercase()
}

fn random_id() -> String {
    uuid::Uuid::now_v7().to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn non_local_http_database_urls_are_rejected() {
        for url in [
            "http://example.com/db",
            "HTTP://example.com/db",
            "http://localhost.evil/db",
            "http://user:pass@example.com/db",
        ] {
            let err = validate_database_url(url).expect_err("reject http");
            assert!(
                err.to_string()
                    .contains("plain HTTP database URLs are only allowed"),
                "{url}: {err:?}"
            );
        }
    }

    #[test]
    fn localhost_http_database_urls_are_allowed() {
        for url in [
            "http://localhost:8080/db",
            "http://LOCALHOST:8080/db/",
            "http://user:pass@localhost:8080/db",
            "http://127.0.0.1:8080/db",
            "http://[::1]:8080/db",
        ] {
            validate_database_url(url).expect(url);
        }
    }

    #[test]
    fn secure_and_file_database_urls_are_allowed() {
        for url in [
            "https://example.com/db",
            "libsql://example.turso.io",
            "file:/tmp/sshoosh.sqlite",
        ] {
            validate_database_url(url).expect(url);
            assert!(is_remote_url(url) || is_file_url(url));
        }
    }

    #[test]
    fn unsupported_database_url_schemes_are_rejected() {
        let err = validate_database_url("ftp://localhost/db").expect_err("reject ftp");
        assert!(err.to_string().contains("unsupported database URL scheme"));
    }
}
