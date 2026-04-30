use std::{
    collections::{HashMap, HashSet},
    sync::Arc,
};

use anyhow::{Context, bail};
use base64::{Engine as _, engine::general_purpose::URL_SAFE_NO_PAD};
use rand::RngCore;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use sqlx::{Column, Row, Sqlite, SqlitePool, Transaction};
use tokio::{
    sync::{RwLock, broadcast, mpsc, oneshot},
    time::{Duration, MissedTickBehavior},
};
use uuid::Uuid;

use crate::db::Database;

include!("service/models.rs");
include!("service/state.rs");
include!("service/accounts.rs");
include!("service/invites_channels.rs");
include!("service/threads.rs");
include!("service/notifications_reactions.rs");
include!("service/webhooks_audit_export.rs");
include!("service/writer.rs");
include!("service/write_ops.rs");
include!("service/permissions.rs");
include!("service/loaders.rs");
include!("service/events.rs");
include!("service/utils.rs");
include!("service/tests.rs");
