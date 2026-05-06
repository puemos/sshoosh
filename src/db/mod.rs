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

use ::time::{OffsetDateTime, format_description::well_known::Rfc3339};
use anyhow::{Context, bail};
use base64::{Engine as _, engine::general_purpose::URL_SAFE_NO_PAD};
use chacha20poly1305::{
    XChaCha20Poly1305, XNonce,
    aead::{Aead, KeyInit, OsRng},
};
use libsql::{Builder, Connection, TransactionBehavior, Value, params_from_iter};
use rand::RngCore;
use secrecy::{ExposeSecret, SecretBox};
use tokio::sync::Mutex;
use url::Url;
use zeroize::Zeroizing;

use crate::features::shared::{label::parse_labels, name::normalize_name_key};

mod config;
mod encryption;
mod executor;
mod fs;
mod lease;
mod maintenance;
mod migrations;
mod time;

pub use config::{DatabaseConfig, DatabaseKind, default_node_id};
use encryption::EncryptionService;
pub use executor::{
    DbExecutor, DbReadSession, DbResult, DbRow, DbTransaction, FromDbRow, FromDbValue, IntoDbValue,
    Query, query, query_as, query_scalar,
};
pub use lease::{DbRole, MasterStatus};
pub use maintenance::{DoctorReport, EncryptionMigrationReport};
pub use time::{format_rfc3339, now};

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

fn random_id() -> String {
    uuid::Uuid::now_v7().to_string()
}
