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

pub const DEFAULT_HISTORY_LIMIT: i64 = 500;
pub const MAX_HISTORY_LIMIT: i64 = 5_000;

#[derive(Clone)]
pub struct ServerState {
    pub db: Database,
    writer: WriteHandle,
    live_tx: broadcast::Sender<LiveEvent>,
    active_connections: Arc<RwLock<HashMap<String, usize>>>,
}

#[derive(Clone)]
pub struct WriteHandle {
    tx: mpsc::Sender<WriteCommand>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct LiveEvent {
    pub seq: i64,
    pub channel_id: Option<String>,
    pub thread_id: Option<String>,
    pub conversation_id: Option<String>,
    pub kind: String,
    pub payload: serde_json::Value,
}

#[derive(Clone, Debug)]
pub struct Account {
    pub id: String,
    pub username: String,
    pub display_name: String,
    pub role: Role,
    pub activated: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Role {
    Owner,
    Admin,
    Member,
}

impl Role {
    pub fn as_str(self) -> &'static str {
        match self {
            Role::Owner => "owner",
            Role::Admin => "admin",
            Role::Member => "member",
        }
    }

    pub fn from_db(value: &str) -> Self {
        match value {
            "owner" => Self::Owner,
            "admin" => Self::Admin,
            _ => Self::Member,
        }
    }

    pub fn can_admin(self) -> bool {
        matches!(self, Role::Owner | Role::Admin)
    }
}

#[derive(Clone, Debug)]
pub struct AccountSummary {
    pub id: String,
    pub username: String,
    pub display_name: String,
    pub role: Role,
    pub activated: bool,
    pub disabled: bool,
    pub created_at: String,
    pub last_seen_at: Option<String>,
}

#[derive(Clone, Debug)]
pub struct SshKeySummary {
    pub id: String,
    pub username: String,
    pub fingerprint: String,
    pub label: Option<String>,
    pub created_at: String,
    pub last_used_at: Option<String>,
    pub revoked_at: Option<String>,
}

#[derive(Clone, Debug)]
pub struct ChannelDirectoryItem {
    pub id: String,
    pub slug: String,
    pub name: String,
    pub visibility: String,
    pub topic: Option<String>,
    pub joined: bool,
    pub archived: bool,
}

#[derive(Clone, Debug)]
pub struct NotificationSummary {
    pub id: String,
    pub kind: String,
    pub actor_username: Option<String>,
    pub title: String,
    pub body: String,
    pub created_at: String,
    pub read_at: Option<String>,
}

#[derive(Clone, Debug)]
pub struct MentionSummary {
    pub id: String,
    pub actor_username: String,
    pub source_kind: String,
    pub title: String,
    pub body: String,
    pub created_at: String,
    pub read_at: Option<String>,
}

#[derive(Clone, Debug)]
pub struct WebhookSummary {
    pub id: String,
    pub name: String,
    pub url: String,
    pub enabled: bool,
    pub created_at: String,
    pub updated_at: String,
    pub disabled_at: Option<String>,
}

#[derive(Clone, Debug)]
pub struct WebhookDeliverySummary {
    pub id: String,
    pub webhook_name: String,
    pub status: String,
    pub attempts: i64,
    pub next_attempt_at: String,
    pub last_error: Option<String>,
    pub created_at: String,
    pub delivered_at: Option<String>,
}

#[derive(Clone, Debug)]
pub struct AuditEntry {
    pub id: String,
    pub actor_username: Option<String>,
    pub action: String,
    pub target: Option<String>,
    pub metadata_json: String,
    pub created_at: String,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ExportFormat {
    Json,
    Markdown,
}

#[derive(Clone, Debug)]
pub struct InviteSummary {
    pub id: String,
    pub role_on_accept: Role,
    pub created_by: String,
    pub accepted_by: Option<String>,
    pub created_at: String,
    pub expires_at: Option<String>,
    pub revoked_at: Option<String>,
    pub accepted_at: Option<String>,
}

#[derive(Clone, Debug)]
pub struct ChannelMemberSummary {
    pub channel_id: String,
    pub channel_slug: String,
    pub username: String,
    pub role: String,
    pub joined_at: String,
}

#[derive(Clone, Debug)]
pub struct UserPresence {
    pub username: String,
    pub display_name: String,
    pub last_seen_at: Option<String>,
    pub connected: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PresenceState {
    Online,
    Away,
    Offline,
}

impl UserPresence {
    pub fn state(&self) -> PresenceState {
        if self.connected {
            return PresenceState::Online;
        }
        let Some(last_seen_at) = self.last_seen_at.as_deref() else {
            return PresenceState::Offline;
        };
        let Ok(last_seen_at) = time::OffsetDateTime::parse(
            last_seen_at,
            &time::format_description::well_known::Rfc3339,
        ) else {
            return PresenceState::Offline;
        };
        let age = time::OffsetDateTime::now_utc() - last_seen_at;
        let age = age.whole_seconds().max(0);
        if age <= 3600 {
            PresenceState::Away
        } else {
            PresenceState::Offline
        }
    }

    pub fn state_label(&self) -> &'static str {
        match self.state() {
            PresenceState::Online => "online",
            PresenceState::Away => "away",
            PresenceState::Offline => "offline",
        }
    }
}

#[derive(Clone, Debug)]
pub struct Channel {
    pub id: String,
    pub slug: String,
    pub name: String,
    pub visibility: String,
    pub topic: Option<String>,
    pub unread_count: i64,
}

#[derive(Clone, Debug)]
pub struct ThreadItem {
    pub id: String,
    pub channel_id: String,
    pub title: String,
    pub body: String,
    pub author: String,
    pub comment_count: i64,
    pub last_comment_index: i64,
    pub unread_count: i64,
    pub last_activity_at: Option<String>,
    pub created_at: String,
    pub edited_at: Option<String>,
    pub archived_at: Option<String>,
    pub pinned_at: Option<String>,
    pub muted_until: Option<String>,
    pub saved_at: Option<String>,
    pub reactions: String,
}

#[derive(Clone, Debug)]
pub struct CommentItem {
    pub id: String,
    pub author: String,
    pub obj_index: i64,
    pub body: String,
    pub created_at: String,
    pub edited_at: Option<String>,
    pub reactions: String,
}

#[derive(Clone, Debug)]
pub struct Conversation {
    pub id: String,
    pub peer_username: String,
    pub last_message_index: i64,
    pub unread_count: i64,
    pub last_activity_at: Option<String>,
    pub last_message_preview: Option<String>,
    pub muted_until: Option<String>,
    pub saved_at: Option<String>,
}

#[derive(Clone, Debug)]
pub struct ConversationMessage {
    pub id: String,
    pub author: String,
    pub obj_index: i64,
    pub body: String,
    pub created_at: String,
    pub edited_at: Option<String>,
    pub reactions: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SearchKind {
    Thread,
    Comment,
    Dm,
}

#[derive(Clone, Debug)]
pub struct SearchResult {
    pub kind: SearchKind,
    pub label: String,
    pub context: String,
    pub snippet: String,
    pub channel_id: Option<String>,
    pub thread_id: Option<String>,
    pub conversation_id: Option<String>,
}

#[derive(Clone, Debug)]
pub struct SearchPage {
    pub results: Vec<SearchResult>,
    pub has_more: bool,
}

#[derive(Clone, Debug, Default)]
pub struct Snapshot {
    pub current_username: Option<String>,
    pub users: Vec<UserPresence>,
    pub channels: Vec<Channel>,
    pub threads: Vec<ThreadItem>,
    pub comments: Vec<CommentItem>,
    pub conversations: Vec<Conversation>,
    pub conversation_messages: Vec<ConversationMessage>,
    pub comments_has_more: bool,
    pub conversation_messages_has_more: bool,
    pub search_query: Option<String>,
    pub search_results: Vec<SearchResult>,
    pub search_has_more: bool,
    pub notifications: Vec<NotificationSummary>,
    pub notification_unread_count: i64,
    pub mention_unread_count: i64,
    pub selected_channel_id: Option<String>,
    pub selected_thread_id: Option<String>,
    pub selected_conversation_id: Option<String>,
}

impl Snapshot {
    pub fn online_user_count(&self) -> usize {
        self.users
            .iter()
            .filter(|user| user.state() == PresenceState::Online)
            .count()
    }

    pub fn presence_for(&self, username: &str) -> PresenceState {
        self.users
            .iter()
            .find(|user| user.username.eq_ignore_ascii_case(username))
            .map(UserPresence::state)
            .unwrap_or(PresenceState::Offline)
    }

    pub fn total_unread(&self) -> i64 {
        self.channels
            .iter()
            .map(|channel| channel.unread_count)
            .sum::<i64>()
            + self
                .conversations
                .iter()
                .map(|conversation| conversation.unread_count)
                .sum::<i64>()
    }

    pub fn channel_unread(&self, channel_id: &str) -> i64 {
        self.channels
            .iter()
            .find(|channel| channel.id == channel_id)
            .map(|channel| channel.unread_count)
            .unwrap_or(0)
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum NextUnread {
    Thread {
        channel_id: String,
        thread_id: String,
    },
    Conversation {
        conversation_id: String,
    },
}

#[derive(Debug)]
enum WriteCommand {
    CreateInvite {
        actor_id: String,
        reply: oneshot::Sender<anyhow::Result<String>>,
    },
    AcceptInvite {
        account_id: String,
        code: String,
        username: String,
        reply: oneshot::Sender<anyhow::Result<()>>,
    },
    CreateChannel {
        actor_id: String,
        name: String,
        private: bool,
        reply: oneshot::Sender<anyhow::Result<String>>,
    },
    JoinChannel {
        actor_id: String,
        slug: String,
        reply: oneshot::Sender<anyhow::Result<String>>,
    },
    CreateThread {
        actor_id: String,
        channel_id: String,
        title: String,
        body: String,
        reply: oneshot::Sender<anyhow::Result<String>>,
    },
    AddComment {
        actor_id: String,
        thread_id: String,
        body: String,
        reply: oneshot::Sender<anyhow::Result<()>>,
    },
    OpenDm {
        actor_id: String,
        target: String,
        reply: oneshot::Sender<anyhow::Result<String>>,
    },
    SendDm {
        actor_id: String,
        conversation_id: String,
        body: String,
        reply: oneshot::Sender<anyhow::Result<()>>,
    },
}

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
        let active_account_ids = self.active_account_ids().await;
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

    pub async fn begin_account_session(&self, account_id: &str) -> anyhow::Result<()> {
        {
            let mut active_connections = self.active_connections.write().await;
            *active_connections
                .entry(account_id.to_string())
                .or_default() += 1;
        }
        if let Err(err) = self.touch_account(account_id).await {
            self.remove_account_session(account_id).await;
            return Err(err);
        }
        Ok(())
    }

    pub async fn end_account_session(&self, account_id: &str) -> anyhow::Result<()> {
        let disconnected = self.remove_account_session(account_id).await;
        if disconnected {
            self.touch_account(account_id).await?;
        }
        Ok(())
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

    pub async fn create_invite_with_options(
        &self,
        actor_id: &str,
        role_on_accept: Role,
        ttl_hours: Option<i64>,
    ) -> anyhow::Result<String> {
        let result = create_invite_with_options(
            self.db.write_pool(),
            &self.live_tx,
            actor_id,
            role_on_accept,
            ttl_hours,
        )
        .await;
        if let Ok(code) = &result {
            let _ = code;
        }
        result
    }

    pub async fn list_accounts(&self, actor_id: &str) -> anyhow::Result<Vec<AccountSummary>> {
        let mut tx = begin(self.db.write_pool()).await?;
        require_admin_tx(&mut tx, actor_id).await?;
        let rows = sqlx::query(
            "SELECT id, username, display_name, role, activated_at, disabled_at, created_at, last_seen_at
             FROM accounts
             ORDER BY username",
        )
        .fetch_all(&mut *tx)
        .await?;
        tx.commit().await?;
        Ok(rows
            .into_iter()
            .map(|row| AccountSummary {
                id: row.get("id"),
                username: row.get("username"),
                display_name: row.get("display_name"),
                role: Role::from_db(row.get::<String, _>("role").as_str()),
                activated: row.get::<Option<String>, _>("activated_at").is_some(),
                disabled: row.get::<Option<String>, _>("disabled_at").is_some(),
                created_at: row.get("created_at"),
                last_seen_at: row.get("last_seen_at"),
            })
            .collect())
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
        sqlx::query("UPDATE accounts SET disabled_at = ?, updated_at = ? WHERE id = ?")
            .bind(if disabled { Some(now.clone()) } else { None })
            .bind(&now)
            .bind(&target.id)
            .execute(&mut *tx)
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
        let event = insert_event(
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
        publish(&self.live_tx, event);
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
        sqlx::query("UPDATE accounts SET role = ?, updated_at = ? WHERE id = ?")
            .bind(role.as_str())
            .bind(&now)
            .bind(&target.id)
            .execute(&mut *tx)
            .await?;
        insert_audit(
            &mut tx,
            Some(actor_id),
            "user.role_changed",
            Some(&target.id),
            serde_json::json!({"username": target.username, "role": role.as_str()}),
        )
        .await?;
        let event = insert_event(
            &mut tx,
            None,
            None,
            None,
            "user.role_changed",
            serde_json::json!({"account_id": target.id, "username": target.username, "role": role.as_str()}),
        )
        .await?;
        tx.commit().await?;
        publish(&self.live_tx, event);
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
        let existing: Option<String> = sqlx::query_scalar(
            "SELECT id FROM accounts WHERE lower(username) = lower(?) AND id <> ?",
        )
        .bind(&next_username)
        .bind(&target.id)
        .fetch_optional(&mut *tx)
        .await?;
        anyhow::ensure!(existing.is_none(), "Username is already taken");
        let now = now();
        sqlx::query("UPDATE accounts SET username = ?, updated_at = ? WHERE id = ?")
            .bind(&next_username)
            .bind(&now)
            .bind(&target.id)
            .execute(&mut *tx)
            .await?;
        insert_audit(
            &mut tx,
            Some(actor_id),
            "user.renamed",
            Some(&target.id),
            serde_json::json!({"from": target.username, "to": next_username}),
        )
        .await?;
        let event = insert_event(
            &mut tx,
            None,
            None,
            None,
            "user.renamed",
            serde_json::json!({"account_id": target.id, "username": next_username}),
        )
        .await?;
        tx.commit().await?;
        publish(&self.live_tx, event);
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
        sqlx::query("UPDATE accounts SET display_name = ?, updated_at = ? WHERE id = ?")
            .bind(display_name)
            .bind(&now)
            .bind(&target.id)
            .execute(&mut *tx)
            .await?;
        insert_audit(
            &mut tx,
            Some(actor_id),
            "user.display_name_changed",
            Some(&target.id),
            serde_json::json!({"username": target.username, "display_name": display_name}),
        )
        .await?;
        let event = insert_event(
            &mut tx,
            None,
            None,
            None,
            "user.display_name_changed",
            serde_json::json!({"account_id": target.id, "display_name": display_name}),
        )
        .await?;
        tx.commit().await?;
        publish(&self.live_tx, event);
        Ok(())
    }

    pub async fn list_my_ssh_keys(&self, account_id: &str) -> anyhow::Result<Vec<SshKeySummary>> {
        let rows = sqlx::query(
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
        sqlx::query(
            "INSERT INTO ssh_keys (id, account_id, fingerprint, public_key, label, created_at)
             VALUES (?, ?, ?, ?, ?, ?)",
        )
        .bind(&key_id)
        .bind(&target.id)
        .bind(&parsed.fingerprint)
        .bind(&parsed.public_key)
        .bind(label.map(str::trim).filter(|value| !value.is_empty()))
        .bind(&now)
        .execute(&mut *tx)
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
        let event = insert_event(
            &mut tx,
            None,
            None,
            None,
            "ssh_key.added",
            serde_json::json!({"key_id": key_id, "account_id": target.id}),
        )
        .await?;
        tx.commit().await?;
        publish(&self.live_tx, event);
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
        let row = sqlx::query(
            "SELECT k.id, k.account_id, a.username, a.role
             FROM ssh_keys k
             JOIN accounts a ON a.id = k.account_id
             WHERE (k.id LIKE ? OR k.fingerprint = ?) AND k.revoked_at IS NULL",
        )
        .bind(format!("{}%", key.trim()))
        .bind(key.trim())
        .fetch_optional(&mut *tx)
        .await?;
        let Some(row) = row else {
            bail!("Active SSH key not found");
        };
        let target = Account {
            id: row.get("account_id"),
            username: row.get("username"),
            display_name: String::new(),
            role: Role::from_db(row.get::<String, _>("role").as_str()),
            activated: true,
        };
        if actor.id != target.id {
            ensure_can_manage_account(&actor, &target)?;
        }
        let key_id: String = row.get("id");
        let label = label.trim();
        let label = (!label.is_empty()).then_some(label);
        sqlx::query("UPDATE ssh_keys SET label = ? WHERE id = ?")
            .bind(label)
            .bind(&key_id)
            .execute(&mut *tx)
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
        let row = sqlx::query(
            "SELECT k.id, k.account_id, a.username AS old_username
             FROM ssh_keys k
             JOIN accounts a ON a.id = k.account_id
             WHERE (k.id LIKE ? OR k.fingerprint = ?) AND k.revoked_at IS NULL",
        )
        .bind(format!("{}%", key.trim()))
        .bind(key.trim())
        .fetch_optional(&mut *tx)
        .await?;
        let Some(row) = row else {
            bail!("Active SSH key not found");
        };
        let key_id: String = row.get("id");
        let old_account_id: String = row.get("account_id");
        sqlx::query("UPDATE ssh_keys SET account_id = ? WHERE id = ?")
            .bind(&target.id)
            .bind(&key_id)
            .execute(&mut *tx)
            .await?;
        let remaining_keys: i64 = sqlx::query_scalar(
            "SELECT COUNT(*) FROM ssh_keys WHERE account_id = ? AND revoked_at IS NULL",
        )
        .bind(&old_account_id)
        .fetch_one(&mut *tx)
        .await?;
        if remaining_keys == 0 {
            let now = now();
            sqlx::query("UPDATE accounts SET disabled_at = ?, updated_at = ? WHERE id = ? AND activated_at IS NULL")
                .bind(&now)
                .bind(&now)
                .bind(&old_account_id)
                .execute(&mut *tx)
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
        let event = insert_event(
            &mut tx,
            None,
            None,
            None,
            "ssh_key.attached",
            serde_json::json!({"key_id": key_id, "account_id": target.id}),
        )
        .await?;
        tx.commit().await?;
        publish(&self.live_tx, event);
        Ok(())
    }

    pub async fn list_ssh_keys(&self, actor_id: &str) -> anyhow::Result<Vec<SshKeySummary>> {
        let mut tx = begin(self.db.write_pool()).await?;
        require_admin_tx(&mut tx, actor_id).await?;
        let rows = sqlx::query(
            "SELECT k.id, a.username, k.fingerprint, k.label, k.created_at, k.last_used_at, k.revoked_at
             FROM ssh_keys k
             JOIN accounts a ON a.id = k.account_id
             ORDER BY a.username, k.created_at",
        )
        .fetch_all(&mut *tx)
        .await?;
        tx.commit().await?;
        Ok(rows.into_iter().map(ssh_key_summary_from_row).collect())
    }

    pub async fn revoke_ssh_key(&self, actor_id: &str, key: &str) -> anyhow::Result<()> {
        let mut tx = begin(self.db.write_pool()).await?;
        let actor = load_account_tx(&mut tx, actor_id).await?;
        let row = sqlx::query(
            "SELECT k.id, k.account_id, k.fingerprint, a.username, a.role
             FROM ssh_keys k
             JOIN accounts a ON a.id = k.account_id
             WHERE (k.id LIKE ? OR k.fingerprint = ?) AND k.revoked_at IS NULL",
        )
        .bind(format!("{}%", key.trim()))
        .bind(key.trim())
        .fetch_optional(&mut *tx)
        .await?;
        let Some(row) = row else {
            bail!("Active SSH key not found");
        };
        let target = Account {
            id: row.get("account_id"),
            username: row.get("username"),
            display_name: String::new(),
            role: Role::from_db(row.get::<String, _>("role").as_str()),
            activated: true,
        };
        if actor.id != target.id {
            ensure_can_manage_account(&actor, &target)?;
        }
        if target.role == Role::Owner {
            ensure_owner_keeps_active_key(&mut tx, &target.id).await?;
        }
        let key_id: String = row.get("id");
        let now = now();
        sqlx::query("UPDATE ssh_keys SET revoked_at = ? WHERE id = ?")
            .bind(&now)
            .bind(&key_id)
            .execute(&mut *tx)
            .await?;
        insert_audit(
            &mut tx,
            Some(actor_id),
            "ssh_key.revoked",
            Some(&key_id),
            serde_json::json!({"username": target.username, "fingerprint": row.get::<String, _>("fingerprint")}),
        )
        .await?;
        let event = insert_event(
            &mut tx,
            None,
            None,
            None,
            "ssh_key.revoked",
            serde_json::json!({"key_id": key_id, "account_id": target.id}),
        )
        .await?;
        tx.commit().await?;
        publish(&self.live_tx, event);
        Ok(())
    }

    pub async fn list_invites(&self, actor_id: &str) -> anyhow::Result<Vec<InviteSummary>> {
        let mut tx = begin(self.db.write_pool()).await?;
        require_admin_tx(&mut tx, actor_id).await?;
        let rows = sqlx::query(
            "SELECT i.id, i.role_on_accept, creator.username AS created_by,
                    accepted.username AS accepted_by, i.created_at, i.expires_at,
                    i.revoked_at, i.accepted_at
             FROM invites i
             JOIN accounts creator ON creator.id = i.created_by_account_id
             LEFT JOIN accounts accepted ON accepted.id = i.accepted_by_account_id
             ORDER BY i.created_at DESC",
        )
        .fetch_all(&mut *tx)
        .await?;
        tx.commit().await?;
        Ok(rows
            .into_iter()
            .map(|row| InviteSummary {
                id: row.get("id"),
                role_on_accept: Role::from_db(row.get::<String, _>("role_on_accept").as_str()),
                created_by: row.get("created_by"),
                accepted_by: row.get("accepted_by"),
                created_at: row.get("created_at"),
                expires_at: row.get("expires_at"),
                revoked_at: row.get("revoked_at"),
                accepted_at: row.get("accepted_at"),
            })
            .collect())
    }

    pub async fn revoke_invite(&self, actor_id: &str, invite_id: &str) -> anyhow::Result<()> {
        let mut tx = begin(self.db.write_pool()).await?;
        require_admin_tx(&mut tx, actor_id).await?;
        let id: Option<String> = sqlx::query_scalar(
            "SELECT id FROM invites
             WHERE id LIKE ? AND accepted_at IS NULL AND revoked_at IS NULL",
        )
        .bind(format!("{}%", invite_id.trim()))
        .fetch_optional(&mut *tx)
        .await?;
        let Some(id) = id else {
            bail!("Open invite not found");
        };
        let now = now();
        sqlx::query("UPDATE invites SET revoked_at = ? WHERE id = ?")
            .bind(&now)
            .bind(&id)
            .execute(&mut *tx)
            .await?;
        insert_audit(
            &mut tx,
            Some(actor_id),
            "invite.revoked",
            Some(&id),
            serde_json::json!({}),
        )
        .await?;
        let event = insert_event(
            &mut tx,
            None,
            None,
            None,
            "invite.revoked",
            serde_json::json!({"invite_id": id}),
        )
        .await?;
        tx.commit().await?;
        publish(&self.live_tx, event);
        Ok(())
    }

    pub async fn list_channel_members(
        &self,
        actor_id: &str,
        slug: &str,
    ) -> anyhow::Result<Vec<ChannelMemberSummary>> {
        let mut tx = begin(self.db.write_pool()).await?;
        let channel = load_channel_by_slug_tx(&mut tx, slug).await?;
        ensure_can_manage_channel(&mut tx, actor_id, &channel).await?;
        let rows = sqlx::query(
            "SELECT c.id AS channel_id, c.slug AS channel_slug, a.username, m.role, m.joined_at
             FROM channel_members m
             JOIN channels c ON c.id = m.channel_id
             JOIN accounts a ON a.id = m.account_id
             WHERE m.channel_id = ?
             ORDER BY a.username",
        )
        .bind(&channel.id)
        .fetch_all(&mut *tx)
        .await?;
        tx.commit().await?;
        Ok(rows
            .into_iter()
            .map(|row| ChannelMemberSummary {
                channel_id: row.get("channel_id"),
                channel_slug: row.get("channel_slug"),
                username: row.get("username"),
                role: row.get("role"),
                joined_at: row.get("joined_at"),
            })
            .collect())
    }

    pub async fn add_channel_member(
        &self,
        actor_id: &str,
        slug: &str,
        username: &str,
    ) -> anyhow::Result<()> {
        update_channel_member(
            self.db.write_pool(),
            &self.live_tx,
            actor_id,
            slug,
            username,
            true,
        )
        .await
    }

    pub async fn remove_channel_member(
        &self,
        actor_id: &str,
        slug: &str,
        username: &str,
    ) -> anyhow::Result<()> {
        update_channel_member(
            self.db.write_pool(),
            &self.live_tx,
            actor_id,
            slug,
            username,
            false,
        )
        .await
    }

    pub async fn list_channels(
        &self,
        actor_id: &str,
        include_archived: bool,
    ) -> anyhow::Result<Vec<ChannelDirectoryItem>> {
        let actor = self.reload_account(actor_id).await?;
        anyhow::ensure!(actor.activated, "Account is not activated");
        let rows = sqlx::query(
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
             ORDER BY CASE WHEN c.slug = 'general' THEN 0 ELSE 1 END, c.slug",
        )
        .bind(actor_id)
        .bind(include_archived)
        .bind(actor_id)
        .bind(actor.role.as_str())
        .fetch_all(self.db.read_pool())
        .await?;
        Ok(rows
            .into_iter()
            .map(|row| ChannelDirectoryItem {
                id: row.get("id"),
                slug: row.get("slug"),
                name: row.get("name"),
                visibility: row.get("visibility"),
                topic: row.get("topic"),
                joined: row.get::<i64, _>("joined") != 0,
                archived: row.get::<Option<String>, _>("archived_at").is_some(),
            })
            .collect())
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
        sqlx::query("DELETE FROM channel_members WHERE channel_id = ? AND account_id = ?")
            .bind(&channel.id)
            .bind(actor_id)
            .execute(&mut *tx)
            .await?;
        insert_audit(
            &mut tx,
            Some(actor_id),
            "channel.left",
            Some(&channel.id),
            serde_json::json!({"channel": channel.slug}),
        )
        .await?;
        let event = insert_event(
            &mut tx,
            Some(&channel.id),
            None,
            None,
            "channel.left",
            serde_json::json!({"channel_id": channel.id, "account_id": actor_id}),
        )
        .await?;
        tx.commit().await?;
        publish(&self.live_tx, event);
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
        sqlx::query("UPDATE channels SET slug = ?, name = ?, updated_at = ? WHERE id = ?")
            .bind(&next_slug)
            .bind(&next_slug)
            .bind(&now)
            .bind(&channel.id)
            .execute(&mut *tx)
            .await?;
        insert_audit(
            &mut tx,
            Some(actor_id),
            "channel.renamed",
            Some(&channel.id),
            serde_json::json!({"from": channel.slug, "to": next_slug}),
        )
        .await?;
        let event = insert_event(
            &mut tx,
            Some(&channel.id),
            None,
            None,
            "channel.renamed",
            serde_json::json!({"channel_id": channel.id, "slug": next_slug}),
        )
        .await?;
        tx.commit().await?;
        publish(&self.live_tx, event);
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
        let topic = topic.trim();
        let topic = (!topic.is_empty()).then_some(topic);
        let now = now();
        sqlx::query("UPDATE channels SET topic = ?, updated_at = ? WHERE id = ?")
            .bind(topic)
            .bind(&now)
            .bind(&channel.id)
            .execute(&mut *tx)
            .await?;
        insert_audit(
            &mut tx,
            Some(actor_id),
            "channel.topic_changed",
            Some(&channel.id),
            serde_json::json!({"channel": channel.slug, "topic": topic}),
        )
        .await?;
        let event = insert_event(
            &mut tx,
            Some(&channel.id),
            None,
            None,
            "channel.topic_changed",
            serde_json::json!({"channel_id": channel.id}),
        )
        .await?;
        tx.commit().await?;
        publish(&self.live_tx, event);
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
        sqlx::query("UPDATE channels SET archived_at = ?, archived_by_account_id = ?, updated_at = ? WHERE id = ?")
            .bind(archived.then_some(now.as_str()))
            .bind(archived.then_some(actor_id))
            .bind(&now)
            .bind(&channel.id)
            .execute(&mut *tx)
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
        let event = insert_event(
            &mut tx,
            Some(&channel.id),
            None,
            None,
            action,
            serde_json::json!({"channel_id": channel.id}),
        )
        .await?;
        tx.commit().await?;
        publish(&self.live_tx, event);
        Ok(())
    }

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

    pub async fn list_notifications(
        &self,
        account_id: &str,
        limit: i64,
    ) -> anyhow::Result<Vec<NotificationSummary>> {
        load_notifications(self.db.read_pool(), account_id, limit).await
    }

    pub async fn mark_notification_read(
        &self,
        account_id: &str,
        notification_id: Option<&str>,
    ) -> anyhow::Result<()> {
        let now = now();
        if let Some(notification_id) = notification_id {
            sqlx::query(
                "UPDATE notifications SET read_at = ?
                 WHERE account_id = ? AND (id = ? OR id LIKE ?)",
            )
            .bind(&now)
            .bind(account_id)
            .bind(notification_id)
            .bind(format!("{}%", notification_id.trim()))
            .execute(self.db.write_pool())
            .await?;
        } else {
            sqlx::query(
                "UPDATE notifications SET read_at = ? WHERE account_id = ? AND read_at IS NULL",
            )
            .bind(&now)
            .bind(account_id)
            .execute(self.db.write_pool())
            .await?;
        }
        Ok(())
    }

    pub async fn list_mentions(
        &self,
        account_id: &str,
        limit: i64,
    ) -> anyhow::Result<Vec<MentionSummary>> {
        let limit = limit.clamp(1, 200);
        let rows = sqlx::query(
            "SELECT m.id, actor.username AS actor_username, m.source_kind,
                    COALESCE(t.title, 'DM') AS title,
                    COALESCE(cm.body, dm.body, t.body, '') AS body,
                    m.created_at, m.read_at
             FROM mentions m
             JOIN accounts actor ON actor.id = m.actor_account_id
             LEFT JOIN threads t ON t.id = m.thread_id
             LEFT JOIN comments cm ON cm.id = m.source_id AND m.source_kind = 'comment'
             LEFT JOIN conversation_messages dm ON dm.id = m.source_id AND m.source_kind = 'dm'
             WHERE m.target_account_id = ?
             ORDER BY m.created_at DESC
             LIMIT ?",
        )
        .bind(account_id)
        .bind(limit)
        .fetch_all(self.db.read_pool())
        .await?;
        Ok(rows
            .into_iter()
            .map(|row| MentionSummary {
                id: row.get("id"),
                actor_username: row.get("actor_username"),
                source_kind: row.get("source_kind"),
                title: row.get("title"),
                body: row.get("body"),
                created_at: row.get("created_at"),
                read_at: row.get("read_at"),
            })
            .collect())
    }

    pub async fn react_to_thread(
        &self,
        account_id: &str,
        thread_id: &str,
        emoji: &str,
        remove: bool,
    ) -> anyhow::Result<()> {
        let mut tx = begin(self.db.write_pool()).await?;
        let thread = load_thread_meta_tx(&mut tx, thread_id).await?;
        ensure_can_view_channel(&mut tx, account_id, &thread.channel_id).await?;
        set_reaction_tx(&mut tx, account_id, "thread", thread_id, emoji, remove).await?;
        let event = insert_event(
            &mut tx,
            Some(&thread.channel_id),
            Some(thread_id),
            None,
            if remove {
                "reaction.removed"
            } else {
                "reaction.added"
            },
            serde_json::json!({"source_kind": "thread", "source_id": thread_id}),
        )
        .await?;
        tx.commit().await?;
        publish(&self.live_tx, event);
        Ok(())
    }

    pub async fn react_to_comment(
        &self,
        account_id: &str,
        thread_id: &str,
        obj_index: i64,
        emoji: &str,
        remove: bool,
    ) -> anyhow::Result<()> {
        let mut tx = begin(self.db.write_pool()).await?;
        let thread = load_thread_meta_tx(&mut tx, thread_id).await?;
        ensure_can_view_channel(&mut tx, account_id, &thread.channel_id).await?;
        let comment = load_comment_meta_tx(&mut tx, thread_id, obj_index).await?;
        set_reaction_tx(&mut tx, account_id, "comment", &comment.id, emoji, remove).await?;
        let event = insert_event(
            &mut tx,
            Some(&thread.channel_id),
            Some(thread_id),
            None,
            if remove {
                "reaction.removed"
            } else {
                "reaction.added"
            },
            serde_json::json!({"source_kind": "comment", "source_id": comment.id}),
        )
        .await?;
        tx.commit().await?;
        publish(&self.live_tx, event);
        Ok(())
    }

    pub async fn react_to_dm(
        &self,
        account_id: &str,
        conversation_id: &str,
        obj_index: i64,
        emoji: &str,
        remove: bool,
    ) -> anyhow::Result<()> {
        let mut tx = begin(self.db.write_pool()).await?;
        let message =
            load_dm_message_meta_tx(&mut tx, account_id, conversation_id, obj_index).await?;
        set_reaction_tx(&mut tx, account_id, "dm", &message.id, emoji, remove).await?;
        let event = insert_event(
            &mut tx,
            None,
            None,
            Some(conversation_id),
            if remove {
                "reaction.removed"
            } else {
                "reaction.added"
            },
            serde_json::json!({"source_kind": "dm", "source_id": message.id}),
        )
        .await?;
        tx.commit().await?;
        publish(&self.live_tx, event);
        Ok(())
    }

    pub async fn list_webhooks(
        &self,
        actor_id: &str,
    ) -> anyhow::Result<(Vec<WebhookSummary>, Vec<WebhookDeliverySummary>)> {
        let mut tx = begin(self.db.write_pool()).await?;
        require_admin_tx(&mut tx, actor_id).await?;
        let webhook_rows = sqlx::query(
            "SELECT id, name, url, enabled, created_at, updated_at, disabled_at
             FROM webhook_subscriptions
             ORDER BY created_at DESC",
        )
        .fetch_all(&mut *tx)
        .await?;
        let delivery_rows = sqlx::query(
            "SELECT j.id, w.name AS webhook_name, j.status, j.attempts, j.next_attempt_at,
                    j.last_error, j.created_at, j.delivered_at
             FROM webhook_jobs j
             JOIN webhook_subscriptions w ON w.id = j.webhook_id
             ORDER BY j.created_at DESC
             LIMIT 50",
        )
        .fetch_all(&mut *tx)
        .await?;
        tx.commit().await?;
        Ok((
            webhook_rows
                .into_iter()
                .map(|row| WebhookSummary {
                    id: row.get("id"),
                    name: row.get("name"),
                    url: row.get("url"),
                    enabled: row.get::<i64, _>("enabled") != 0,
                    created_at: row.get("created_at"),
                    updated_at: row.get("updated_at"),
                    disabled_at: row.get("disabled_at"),
                })
                .collect(),
            delivery_rows
                .into_iter()
                .map(|row| WebhookDeliverySummary {
                    id: row.get("id"),
                    webhook_name: row.get("webhook_name"),
                    status: row.get("status"),
                    attempts: row.get("attempts"),
                    next_attempt_at: row.get("next_attempt_at"),
                    last_error: row.get("last_error"),
                    created_at: row.get("created_at"),
                    delivered_at: row.get("delivered_at"),
                })
                .collect(),
        ))
    }

    pub async fn add_webhook(
        &self,
        actor_id: &str,
        name: &str,
        url: &str,
    ) -> anyhow::Result<String> {
        anyhow::ensure!(
            url.starts_with("http://") || url.starts_with("https://"),
            "Webhook URL must be http(s)"
        );
        let mut tx = begin(self.db.write_pool()).await?;
        require_admin_tx(&mut tx, actor_id).await?;
        let now = now();
        let id = id();
        sqlx::query(
            "INSERT INTO webhook_subscriptions
             (id, created_by_account_id, name, url, enabled, created_at, updated_at)
             VALUES (?, ?, ?, ?, 1, ?, ?)",
        )
        .bind(&id)
        .bind(actor_id)
        .bind(name.trim())
        .bind(url.trim())
        .bind(&now)
        .bind(&now)
        .execute(&mut *tx)
        .await?;
        insert_audit(
            &mut tx,
            Some(actor_id),
            "webhook.added",
            Some(&id),
            serde_json::json!({"name": name.trim(), "url": url.trim()}),
        )
        .await?;
        tx.commit().await?;
        Ok(id)
    }

    pub async fn remove_webhook(&self, actor_id: &str, webhook: &str) -> anyhow::Result<()> {
        let mut tx = begin(self.db.write_pool()).await?;
        require_admin_tx(&mut tx, actor_id).await?;
        let row = sqlx::query(
            "SELECT id, name FROM webhook_subscriptions WHERE id LIKE ? AND disabled_at IS NULL",
        )
        .bind(format!("{}%", webhook.trim()))
        .fetch_optional(&mut *tx)
        .await?;
        let Some(row) = row else {
            bail!("Active webhook not found");
        };
        let id: String = row.get("id");
        let now = now();
        sqlx::query("UPDATE webhook_subscriptions SET enabled = 0, disabled_at = ?, updated_at = ? WHERE id = ?")
            .bind(&now)
            .bind(&now)
            .bind(&id)
            .execute(&mut *tx)
            .await?;
        insert_audit(
            &mut tx,
            Some(actor_id),
            "webhook.removed",
            Some(&id),
            serde_json::json!({"name": row.get::<String, _>("name")}),
        )
        .await?;
        tx.commit().await?;
        Ok(())
    }

    pub async fn test_webhook(&self, actor_id: &str, webhook: &str) -> anyhow::Result<()> {
        let mut tx = begin(self.db.write_pool()).await?;
        require_admin_tx(&mut tx, actor_id).await?;
        let row = sqlx::query("SELECT id, name FROM webhook_subscriptions WHERE id LIKE ? AND enabled = 1 AND disabled_at IS NULL")
            .bind(format!("{}%", webhook.trim()))
            .fetch_optional(&mut *tx)
            .await?;
        let Some(row) = row else {
            bail!("Active webhook not found");
        };
        let webhook_id: String = row.get("id");
        let webhook_name: String = row.get("name");
        let now = now();
        let payload = serde_json::json!({
            "kind": "webhook_test",
            "title": "sshoosh webhook test",
            "body": "Webhook delivery test",
        });
        sqlx::query(
            "INSERT INTO webhook_jobs
             (id, webhook_id, payload_json, status, attempts, next_attempt_at, created_at, updated_at)
             VALUES (?, ?, ?, 'pending', 0, ?, ?, ?)",
        )
        .bind(id())
        .bind(&webhook_id)
        .bind(serde_json::to_string(&payload)?)
        .bind(&now)
        .bind(&now)
        .bind(&now)
        .execute(&mut *tx)
        .await?;
        insert_audit(
            &mut tx,
            Some(actor_id),
            "webhook.test_queued",
            Some(&webhook_id),
            serde_json::json!({"name": webhook_name}),
        )
        .await?;
        tx.commit().await?;
        Ok(())
    }

    pub async fn list_audit(&self, actor_id: &str, limit: i64) -> anyhow::Result<Vec<AuditEntry>> {
        let mut tx = begin(self.db.write_pool()).await?;
        require_admin_tx(&mut tx, actor_id).await?;
        let rows = sqlx::query(
            "SELECT l.id, actor.username AS actor_username, l.action, l.target,
                    l.metadata_json, l.created_at
             FROM audit_log l
             LEFT JOIN accounts actor ON actor.id = l.actor_account_id
             ORDER BY l.created_at DESC
             LIMIT ?",
        )
        .bind(limit.clamp(1, 500))
        .fetch_all(&mut *tx)
        .await?;
        tx.commit().await?;
        Ok(rows
            .into_iter()
            .map(|row| AuditEntry {
                id: row.get("id"),
                actor_username: row.get("actor_username"),
                action: row.get("action"),
                target: row.get("target"),
                metadata_json: row.get("metadata_json"),
                created_at: row.get("created_at"),
            })
            .collect())
    }

    pub async fn export_workspace(
        &self,
        actor_id: &str,
        format: ExportFormat,
        include_audit: bool,
    ) -> anyhow::Result<String> {
        let mut tx = begin(self.db.write_pool()).await?;
        require_admin_tx(&mut tx, actor_id).await?;
        let accounts = rows_to_json(
            sqlx::query("SELECT id, username, display_name, role, created_at, activated_at, disabled_at FROM accounts ORDER BY username")
                .fetch_all(&mut *tx)
                .await?,
        )?;
        let channels = rows_to_json(
            sqlx::query("SELECT id, slug, name, visibility, topic, created_at, archived_at FROM channels ORDER BY slug")
                .fetch_all(&mut *tx)
                .await?,
        )?;
        let threads = rows_to_json(
            sqlx::query("SELECT id, channel_id, creator_account_id, title, body, comment_count, created_at, edited_at, archived_at, deleted_at FROM threads ORDER BY created_at")
                .fetch_all(&mut *tx)
                .await?,
        )?;
        let comments = rows_to_json(
            sqlx::query("SELECT id, thread_id, channel_id, author_account_id, obj_index, body, created_at, edited_at, deleted_at FROM comments ORDER BY created_at")
                .fetch_all(&mut *tx)
                .await?,
        )?;
        let dms = rows_to_json(
            sqlx::query("SELECT id, dm_key, creator_account_id, created_at, archived_at FROM conversations ORDER BY created_at")
                .fetch_all(&mut *tx)
                .await?,
        )?;
        let dm_messages = rows_to_json(
            sqlx::query("SELECT id, conversation_id, author_account_id, obj_index, body, created_at, edited_at, deleted_at FROM conversation_messages ORDER BY created_at")
                .fetch_all(&mut *tx)
                .await?,
        )?;
        let mentions = rows_to_json(
            sqlx::query("SELECT * FROM mentions ORDER BY created_at")
                .fetch_all(&mut *tx)
                .await?,
        )?;
        let reactions = rows_to_json(
            sqlx::query("SELECT * FROM reactions ORDER BY created_at")
                .fetch_all(&mut *tx)
                .await?,
        )?;
        let notifications = rows_to_json(
            sqlx::query("SELECT * FROM notifications ORDER BY created_at")
                .fetch_all(&mut *tx)
                .await?,
        )?;
        let webhooks = rows_to_json(
            sqlx::query("SELECT id, name, url, enabled, created_at, updated_at, disabled_at FROM webhook_subscriptions ORDER BY created_at")
                .fetch_all(&mut *tx)
                .await?,
        )?;
        let audit = if include_audit {
            rows_to_json(
                sqlx::query("SELECT * FROM audit_log ORDER BY created_at")
                    .fetch_all(&mut *tx)
                    .await?,
            )?
        } else {
            serde_json::Value::Array(Vec::new())
        };
        tx.commit().await?;
        let bundle = serde_json::json!({
            "exported_at": now(),
            "users": accounts,
            "channels": channels,
            "threads": threads,
            "comments": comments,
            "dms": dms,
            "dm_messages": dm_messages,
            "mentions": mentions,
            "reactions": reactions,
            "notifications": notifications,
            "webhooks": webhooks,
            "audit": audit,
        });
        match format {
            ExportFormat::Json => Ok(serde_json::to_string_pretty(&bundle)?),
            ExportFormat::Markdown => Ok(export_markdown(&bundle)),
        }
    }

    pub async fn mark_thread_read(&self, account_id: &str, thread_id: &str) -> anyhow::Result<()> {
        let mut tx = begin(self.db.write_pool()).await?;
        let last_index: i64 =
            sqlx::query_scalar("SELECT last_comment_index FROM threads WHERE id = ?")
                .bind(thread_id)
                .fetch_one(&mut *tx)
                .await?;
        sqlx::query(
            "INSERT INTO thread_reads (thread_id, account_id, last_read_index, marked_unread_at)
             VALUES (?, ?, ?, NULL)
             ON CONFLICT(thread_id, account_id)
             DO UPDATE SET last_read_index = excluded.last_read_index, marked_unread_at = NULL",
        )
        .bind(thread_id)
        .bind(account_id)
        .bind(last_index)
        .execute(&mut *tx)
        .await?;
        let now = now();
        sqlx::query(
            "UPDATE notifications SET read_at = COALESCE(read_at, ?)
             WHERE account_id = ? AND thread_id = ?",
        )
        .bind(&now)
        .bind(account_id)
        .bind(thread_id)
        .execute(&mut *tx)
        .await?;
        sqlx::query(
            "UPDATE mentions SET read_at = COALESCE(read_at, ?)
             WHERE target_account_id = ? AND thread_id = ?",
        )
        .bind(&now)
        .bind(account_id)
        .bind(thread_id)
        .execute(&mut *tx)
        .await?;
        tx.commit().await?;
        Ok(())
    }

    pub async fn mark_thread_unread(
        &self,
        account_id: &str,
        thread_id: &str,
    ) -> anyhow::Result<()> {
        let mut tx = begin(self.db.write_pool()).await?;
        let last_index: i64 =
            sqlx::query_scalar("SELECT last_comment_index FROM threads WHERE id = ?")
                .bind(thread_id)
                .fetch_one(&mut *tx)
                .await?;
        let unread_from = last_index.saturating_sub(1);
        sqlx::query(
            "INSERT INTO thread_reads (thread_id, account_id, last_read_index, marked_unread_at)
             VALUES (?, ?, ?, ?)
             ON CONFLICT(thread_id, account_id)
             DO UPDATE SET last_read_index = excluded.last_read_index, marked_unread_at = excluded.marked_unread_at",
        )
        .bind(thread_id)
        .bind(account_id)
        .bind(unread_from)
        .bind(now())
        .execute(&mut *tx)
        .await?;
        tx.commit().await?;
        Ok(())
    }

    pub async fn mark_conversation_read(
        &self,
        account_id: &str,
        conversation_id: &str,
    ) -> anyhow::Result<()> {
        let last_index: i64 =
            sqlx::query_scalar("SELECT last_message_index FROM conversations WHERE id = ?")
                .bind(conversation_id)
                .fetch_one(self.db.read_pool())
                .await?;
        sqlx::query(
            "UPDATE conversation_members SET last_read_index = ? WHERE conversation_id = ? AND account_id = ?",
        )
        .bind(last_index)
        .bind(conversation_id)
        .bind(account_id)
        .execute(self.db.write_pool())
        .await?;
        sqlx::query(
            "UPDATE notifications SET read_at = COALESCE(read_at, ?)
             WHERE account_id = ? AND conversation_id = ?",
        )
        .bind(now())
        .bind(account_id)
        .bind(conversation_id)
        .execute(self.db.write_pool())
        .await?;
        sqlx::query(
            "UPDATE mentions SET read_at = COALESCE(read_at, ?)
             WHERE target_account_id = ? AND conversation_id = ?",
        )
        .bind(now())
        .bind(account_id)
        .bind(conversation_id)
        .execute(self.db.write_pool())
        .await?;
        Ok(())
    }

    pub async fn mark_conversation_unread(
        &self,
        account_id: &str,
        conversation_id: &str,
    ) -> anyhow::Result<()> {
        let last_index: i64 =
            sqlx::query_scalar("SELECT last_message_index FROM conversations WHERE id = ?")
                .bind(conversation_id)
                .fetch_one(self.db.read_pool())
                .await?;
        sqlx::query(
            "UPDATE conversation_members SET last_read_index = ? WHERE conversation_id = ? AND account_id = ?",
        )
        .bind(last_index.saturating_sub(1))
        .bind(conversation_id)
        .bind(account_id)
        .execute(self.db.write_pool())
        .await?;
        Ok(())
    }

    pub async fn next_unread(&self, account_id: &str) -> anyhow::Result<Option<NextUnread>> {
        if let Some(row) = sqlx::query(
            "SELECT t.channel_id, t.id AS thread_id
             FROM threads t
             JOIN channels c ON c.id = t.channel_id
             LEFT JOIN thread_reads r ON r.thread_id = t.id AND r.account_id = ?
             WHERE t.deleted_at IS NULL
               AND t.archived_at IS NULL
               AND (r.muted_until IS NULL OR r.muted_until <= ?)
               AND (
                 SELECT COUNT(*)
                 FROM comments cm
                 WHERE cm.thread_id = t.id
                   AND cm.deleted_at IS NULL
                   AND cm.obj_index > COALESCE(r.last_read_index, 0)
               ) > 0
               AND EXISTS (
                 SELECT 1 FROM channel_members m
                 WHERE m.channel_id = c.id AND m.account_id = ?
               )
             ORDER BY t.last_activity_at DESC
             LIMIT 1",
        )
        .bind(account_id)
        .bind(now())
        .bind(account_id)
        .fetch_optional(self.db.read_pool())
        .await?
        {
            return Ok(Some(NextUnread::Thread {
                channel_id: row.get("channel_id"),
                thread_id: row.get("thread_id"),
            }));
        }

        let conversation_id: Option<String> = sqlx::query_scalar(
            "SELECT c.id
             FROM conversations c
             JOIN conversation_members me ON me.conversation_id = c.id AND me.account_id = ?
             WHERE (
                 SELECT COUNT(*)
                 FROM conversation_messages msg
                 WHERE msg.conversation_id = c.id
                   AND msg.deleted_at IS NULL
                   AND msg.obj_index > me.last_read_index
               ) > 0
               AND c.archived_at IS NULL
               AND (me.muted_until IS NULL OR me.muted_until <= ?)
             ORDER BY c.last_activity_at DESC
             LIMIT 1",
        )
        .bind(account_id)
        .bind(now())
        .fetch_optional(self.db.read_pool())
        .await?;
        Ok(conversation_id.map(|conversation_id| NextUnread::Conversation { conversation_id }))
    }
}

impl WriteHandle {
    async fn request<T>(
        &self,
        build: impl FnOnce(oneshot::Sender<anyhow::Result<T>>) -> WriteCommand,
    ) -> anyhow::Result<T>
    where
        T: Send + 'static,
    {
        let (reply, rx) = oneshot::channel();
        self.tx
            .send(build(reply))
            .await
            .context("writer task is not running")?;
        rx.await.context("writer task dropped response")?
    }

    async fn create_invite(&self, actor_id: String) -> anyhow::Result<String> {
        self.request(|reply| WriteCommand::CreateInvite { actor_id, reply })
            .await
    }

    async fn accept_invite(
        &self,
        account_id: String,
        code: String,
        username: String,
    ) -> anyhow::Result<()> {
        self.request(|reply| WriteCommand::AcceptInvite {
            account_id,
            code,
            username,
            reply,
        })
        .await
    }

    async fn create_channel(
        &self,
        actor_id: String,
        name: String,
        private: bool,
    ) -> anyhow::Result<String> {
        self.request(|reply| WriteCommand::CreateChannel {
            actor_id,
            name,
            private,
            reply,
        })
        .await
    }

    async fn join_channel(&self, actor_id: String, slug: String) -> anyhow::Result<String> {
        self.request(|reply| WriteCommand::JoinChannel {
            actor_id,
            slug,
            reply,
        })
        .await
    }

    async fn create_thread(
        &self,
        actor_id: String,
        channel_id: String,
        title: String,
        body: String,
    ) -> anyhow::Result<String> {
        self.request(|reply| WriteCommand::CreateThread {
            actor_id,
            channel_id,
            title,
            body,
            reply,
        })
        .await
    }

    async fn add_comment(
        &self,
        actor_id: String,
        thread_id: String,
        body: String,
    ) -> anyhow::Result<()> {
        self.request(|reply| WriteCommand::AddComment {
            actor_id,
            thread_id,
            body,
            reply,
        })
        .await
    }

    async fn open_dm(&self, actor_id: String, target: String) -> anyhow::Result<String> {
        self.request(|reply| WriteCommand::OpenDm {
            actor_id,
            target,
            reply,
        })
        .await
    }

    async fn send_dm(
        &self,
        actor_id: String,
        conversation_id: String,
        body: String,
    ) -> anyhow::Result<()> {
        self.request(|reply| WriteCommand::SendDm {
            actor_id,
            conversation_id,
            body,
            reply,
        })
        .await
    }
}

fn start_writer(
    pool: SqlitePool,
    live_tx: broadcast::Sender<LiveEvent>,
    mut rx: mpsc::Receiver<WriteCommand>,
) {
    tokio::spawn(async move {
        while let Some(command) = rx.recv().await {
            let live_tx = live_tx.clone();
            match command {
                WriteCommand::CreateInvite { actor_id, reply } => {
                    let result = create_invite(&pool, &live_tx, &actor_id).await;
                    let _ = reply.send(result);
                }
                WriteCommand::AcceptInvite {
                    account_id,
                    code,
                    username,
                    reply,
                } => {
                    let result =
                        accept_invite(&pool, &live_tx, &account_id, &code, &username).await;
                    let _ = reply.send(result);
                }
                WriteCommand::CreateChannel {
                    actor_id,
                    name,
                    private,
                    reply,
                } => {
                    let result = create_channel(&pool, &live_tx, &actor_id, &name, private).await;
                    let _ = reply.send(result);
                }
                WriteCommand::JoinChannel {
                    actor_id,
                    slug,
                    reply,
                } => {
                    let result = join_channel(&pool, &live_tx, &actor_id, &slug).await;
                    let _ = reply.send(result);
                }
                WriteCommand::CreateThread {
                    actor_id,
                    channel_id,
                    title,
                    body,
                    reply,
                } => {
                    let result =
                        create_thread(&pool, &live_tx, &actor_id, &channel_id, &title, &body).await;
                    let _ = reply.send(result);
                }
                WriteCommand::AddComment {
                    actor_id,
                    thread_id,
                    body,
                    reply,
                } => {
                    let result = add_comment(&pool, &live_tx, &actor_id, &thread_id, &body).await;
                    let _ = reply.send(result);
                }
                WriteCommand::OpenDm {
                    actor_id,
                    target,
                    reply,
                } => {
                    let result = open_dm(&pool, &live_tx, &actor_id, &target).await;
                    let _ = reply.send(result);
                }
                WriteCommand::SendDm {
                    actor_id,
                    conversation_id,
                    body,
                    reply,
                } => {
                    let result = send_dm(&pool, &live_tx, &actor_id, &conversation_id, &body).await;
                    let _ = reply.send(result);
                }
            }
        }
    });
}

fn start_event_poller(
    pool: SqlitePool,
    live_tx: broadcast::Sender<LiveEvent>,
    cursor: Arc<RwLock<i64>>,
) {
    tokio::spawn(async move {
        let mut tick = tokio::time::interval(Duration::from_millis(500));
        tick.set_missed_tick_behavior(MissedTickBehavior::Skip);
        loop {
            tick.tick().await;
            let last_seq = *cursor.read().await;
            let rows = match sqlx::query(
                "SELECT seq, channel_id, thread_id, conversation_id, kind, payload_json
                 FROM event_log
                 WHERE seq > ?
                 ORDER BY seq
                 LIMIT 100",
            )
            .bind(last_seq)
            .fetch_all(&pool)
            .await
            {
                Ok(rows) => rows,
                Err(err) => {
                    tracing::debug!(error = ?err, "event poll failed");
                    continue;
                }
            };
            if rows.is_empty() {
                continue;
            }
            let mut next_seq = last_seq;
            for row in rows {
                let seq: i64 = row.get("seq");
                next_seq = next_seq.max(seq);
                let payload_json: String = row.get("payload_json");
                let payload =
                    serde_json::from_str(&payload_json).unwrap_or(serde_json::Value::Null);
                publish(
                    &live_tx,
                    LiveEvent {
                        seq,
                        channel_id: row.get("channel_id"),
                        thread_id: row.get("thread_id"),
                        conversation_id: row.get("conversation_id"),
                        kind: row.get("kind"),
                        payload,
                    },
                );
            }
            *cursor.write().await = next_seq;
        }
    });
}

fn start_webhook_worker(pool: SqlitePool) {
    tokio::spawn(async move {
        let client = reqwest::Client::new();
        let mut tick = tokio::time::interval(Duration::from_secs(2));
        tick.set_missed_tick_behavior(MissedTickBehavior::Skip);
        loop {
            tick.tick().await;
            if let Err(err) = deliver_due_webhooks(&pool, &client).await {
                tracing::debug!(error = ?err, "webhook delivery sweep failed");
            }
        }
    });
}

async fn deliver_due_webhooks(pool: &SqlitePool, client: &reqwest::Client) -> anyhow::Result<()> {
    let rows = sqlx::query(
        "SELECT j.id, j.payload_json, j.attempts, w.url
         FROM webhook_jobs j
         JOIN webhook_subscriptions w ON w.id = j.webhook_id
         WHERE j.status = 'pending'
           AND j.next_attempt_at <= ?
           AND w.enabled = 1
           AND w.disabled_at IS NULL
         ORDER BY j.created_at
         LIMIT 10",
    )
    .bind(now())
    .fetch_all(pool)
    .await?;
    for row in rows {
        let job_id: String = row.get("id");
        let url: String = row.get("url");
        let payload_json: String = row.get("payload_json");
        let attempts: i64 = row.get("attempts");
        let payload: serde_json::Value = serde_json::from_str(&payload_json)?;
        let result = client.post(&url).json(&payload).send().await;
        let now = now();
        match result {
            Ok(response) if response.status().is_success() => {
                sqlx::query(
                    "UPDATE webhook_jobs
                     SET status = 'delivered', attempts = attempts + 1, updated_at = ?,
                         delivered_at = ?, last_error = NULL
                     WHERE id = ?",
                )
                .bind(&now)
                .bind(&now)
                .bind(&job_id)
                .execute(pool)
                .await?;
            }
            Ok(response) => {
                let status = response.status();
                let next_attempts = attempts + 1;
                let failed = next_attempts >= 8;
                let next_attempt_at = webhook_retry_at(next_attempts);
                sqlx::query(
                    "UPDATE webhook_jobs
                     SET status = ?, attempts = ?, next_attempt_at = ?, last_error = ?, updated_at = ?
                     WHERE id = ?",
                )
                .bind(if failed { "failed" } else { "pending" })
                .bind(next_attempts)
                .bind(&next_attempt_at)
                .bind(format!("HTTP {status}"))
                .bind(&now)
                .bind(&job_id)
                .execute(pool)
                .await?;
            }
            Err(err) => {
                let next_attempts = attempts + 1;
                let failed = next_attempts >= 8;
                let next_attempt_at = webhook_retry_at(next_attempts);
                sqlx::query(
                    "UPDATE webhook_jobs
                     SET status = ?, attempts = ?, next_attempt_at = ?, last_error = ?, updated_at = ?
                     WHERE id = ?",
                )
                .bind(if failed { "failed" } else { "pending" })
                .bind(next_attempts)
                .bind(&next_attempt_at)
                .bind(err.to_string())
                .bind(&now)
                .bind(&job_id)
                .execute(pool)
                .await?;
            }
        }
    }
    Ok(())
}

fn webhook_retry_at(attempts: i64) -> String {
    let seconds = 2_i64
        .saturating_pow(attempts.clamp(0, 6) as u32)
        .saturating_mul(30);
    (time::OffsetDateTime::now_utc() + time::Duration::seconds(seconds))
        .format(&time::format_description::well_known::Rfc3339)
        .expect("format timestamp")
}

async fn create_invite(
    pool: &SqlitePool,
    live_tx: &broadcast::Sender<LiveEvent>,
    actor_id: &str,
) -> anyhow::Result<String> {
    create_invite_with_options(pool, live_tx, actor_id, Role::Member, None).await
}

async fn create_invite_with_options(
    pool: &SqlitePool,
    live_tx: &broadcast::Sender<LiveEvent>,
    actor_id: &str,
    role_on_accept: Role,
    ttl_hours: Option<i64>,
) -> anyhow::Result<String> {
    let mut tx = begin(pool).await?;
    let actor = load_account_tx(&mut tx, actor_id).await?;
    anyhow::ensure!(
        actor.role.can_admin(),
        "Only owners/admins can create invites"
    );
    if role_on_accept == Role::Admin {
        anyhow::ensure!(
            actor.role == Role::Owner,
            "Only owners can create admin invites"
        );
    }
    let code = invite_code();
    let code_hash = code_hash(&code);
    let now = now();
    let expires_at = ttl_hours.and_then(timestamp_after_hours);
    sqlx::query(
        "INSERT INTO invites
         (id, code_hash, role_on_accept, created_by_account_id, created_at, expires_at)
         VALUES (?, ?, ?, ?, ?, ?)",
    )
    .bind(id())
    .bind(code_hash)
    .bind(role_on_accept.as_str())
    .bind(actor_id)
    .bind(&now)
    .bind(expires_at.as_deref())
    .execute(&mut *tx)
    .await?;
    insert_audit(
        &mut tx,
        Some(actor_id),
        "invite.created",
        None,
        serde_json::json!({"role": role_on_accept.as_str(), "expires_at": expires_at}),
    )
    .await?;
    let event = insert_event(
        &mut tx,
        None,
        None,
        None,
        "invite.created",
        serde_json::json!({"actor_id": actor_id, "role": role_on_accept.as_str()}),
    )
    .await?;
    tx.commit().await?;
    publish(live_tx, event);
    Ok(code)
}

async fn accept_invite(
    pool: &SqlitePool,
    live_tx: &broadcast::Sender<LiveEvent>,
    account_id: &str,
    code: &str,
    username: &str,
) -> anyhow::Result<()> {
    let username = normalize_username(username)?;
    let mut tx = begin(pool).await?;
    let account = load_account_tx(&mut tx, account_id).await?;
    if account.activated {
        tx.commit().await?;
        return Ok(());
    }
    let now = now();
    let invite = sqlx::query(
        "SELECT id, role_on_accept
         FROM invites
         WHERE code_hash = ?
           AND accepted_at IS NULL
           AND revoked_at IS NULL
           AND (expires_at IS NULL OR expires_at > ?)",
    )
    .bind(code_hash(code.trim()))
    .bind(&now)
    .fetch_optional(&mut *tx)
    .await?;
    let Some(invite) = invite else {
        bail!("Invite is invalid, expired, or already used");
    };
    let invite_id: String = invite.get("id");
    let role: String = invite.get("role_on_accept");
    let existing: Option<String> =
        sqlx::query_scalar("SELECT id FROM accounts WHERE lower(username) = lower(?) AND id <> ?")
            .bind(&username)
            .bind(account_id)
            .fetch_optional(&mut *tx)
            .await?;
    anyhow::ensure!(existing.is_none(), "Username is already taken");
    sqlx::query(
        "UPDATE accounts
         SET username = ?, display_name = ?, role = ?, activated_at = ?, updated_at = ?
         WHERE id = ?",
    )
    .bind(&username)
    .bind(&username)
    .bind(&role)
    .bind(&now)
    .bind(&now)
    .bind(account_id)
    .execute(&mut *tx)
    .await?;
    sqlx::query("UPDATE invites SET accepted_by_account_id = ?, accepted_at = ? WHERE id = ?")
        .bind(account_id)
        .bind(&now)
        .bind(invite_id)
        .execute(&mut *tx)
        .await?;
    if let Some(general_id) =
        sqlx::query_scalar::<_, String>("SELECT id FROM channels WHERE slug = 'general'")
            .fetch_optional(&mut *tx)
            .await?
    {
        sqlx::query(
            "INSERT INTO channel_members (channel_id, account_id, role, joined_at)
             VALUES (?, ?, 'member', ?)
             ON CONFLICT(channel_id, account_id) DO NOTHING",
        )
        .bind(general_id)
        .bind(account_id)
        .bind(&now)
        .execute(&mut *tx)
        .await?;
    }
    let event = insert_event(
        &mut tx,
        None,
        None,
        None,
        "invite.accepted",
        serde_json::json!({"account_id": account_id, "username": username}),
    )
    .await?;
    tx.commit().await?;
    publish(live_tx, event);
    Ok(())
}

async fn create_channel(
    pool: &SqlitePool,
    live_tx: &broadcast::Sender<LiveEvent>,
    actor_id: &str,
    name: &str,
    private: bool,
) -> anyhow::Result<String> {
    let mut tx = begin(pool).await?;
    let actor = load_account_tx(&mut tx, actor_id).await?;
    anyhow::ensure!(actor.activated, "Account is not activated");
    let slug = normalize_slug(name)?;
    ensure_channel_name_available(&mut tx, &slug).await?;
    let now = now();
    let channel_id = id();
    sqlx::query(
        "INSERT INTO channels
         (id, slug, name, visibility, created_by_account_id, created_at, updated_at)
         VALUES (?, ?, ?, ?, ?, ?, ?)",
    )
    .bind(&channel_id)
    .bind(&slug)
    .bind(&slug)
    .bind(if private { "private" } else { "public" })
    .bind(actor_id)
    .bind(&now)
    .bind(&now)
    .execute(&mut *tx)
    .await?;
    sqlx::query(
        "INSERT INTO channel_members (channel_id, account_id, role, joined_at)
         VALUES (?, ?, 'owner', ?)",
    )
    .bind(&channel_id)
    .bind(actor_id)
    .bind(&now)
    .execute(&mut *tx)
    .await?;
    let event = insert_event(
        &mut tx,
        Some(&channel_id),
        None,
        None,
        "channel.created",
        serde_json::json!({"channel_id": channel_id, "slug": slug, "private": private}),
    )
    .await?;
    tx.commit().await?;
    publish(live_tx, event);
    Ok(channel_id)
}

async fn join_channel(
    pool: &SqlitePool,
    live_tx: &broadcast::Sender<LiveEvent>,
    actor_id: &str,
    slug: &str,
) -> anyhow::Result<String> {
    let mut tx = begin(pool).await?;
    let actor = load_account_tx(&mut tx, actor_id).await?;
    anyhow::ensure!(actor.activated, "Account is not activated");
    let slug = slug.trim().trim_start_matches('#').to_lowercase();
    let row =
        sqlx::query("SELECT id, visibility FROM channels WHERE slug = ? AND archived_at IS NULL")
            .bind(&slug)
            .fetch_optional(&mut *tx)
            .await?;
    let Some(row) = row else {
        bail!("Channel #{slug} not found");
    };
    let channel_id: String = row.get("id");
    let visibility: String = row.get("visibility");
    anyhow::ensure!(visibility == "public", "Private channels require an invite");
    let now = now();
    sqlx::query(
        "INSERT INTO channel_members (channel_id, account_id, role, joined_at)
         VALUES (?, ?, 'member', ?)
         ON CONFLICT(channel_id, account_id) DO NOTHING",
    )
    .bind(&channel_id)
    .bind(actor_id)
    .bind(&now)
    .execute(&mut *tx)
    .await?;
    let event = insert_event(
        &mut tx,
        Some(&channel_id),
        None,
        None,
        "channel.member_added",
        serde_json::json!({"channel_id": channel_id, "account_id": actor_id}),
    )
    .await?;
    tx.commit().await?;
    publish(live_tx, event);
    Ok(channel_id)
}

async fn create_thread(
    pool: &SqlitePool,
    live_tx: &broadcast::Sender<LiveEvent>,
    actor_id: &str,
    channel_id: &str,
    title: &str,
    _body: &str,
) -> anyhow::Result<String> {
    let title = title.trim();
    anyhow::ensure!(!title.is_empty(), "Thread title is required");
    let body = "";
    let title_key = normalize_name_key(title);
    anyhow::ensure!(
        !title_key.is_empty(),
        "Thread title must contain letters or numbers"
    );
    let mut tx = begin(pool).await?;
    ensure_can_view_channel(&mut tx, actor_id, channel_id).await?;
    ensure_thread_name_available(&mut tx, channel_id, &title_key).await?;
    let now = now();
    let thread_id = id();
    sqlx::query(
        "INSERT INTO threads
         (id, channel_id, creator_account_id, title, body, last_activity_at, created_at, updated_at)
         VALUES (?, ?, ?, ?, ?, ?, ?, ?)",
    )
    .bind(&thread_id)
    .bind(channel_id)
    .bind(actor_id)
    .bind(title)
    .bind(body)
    .bind(&now)
    .bind(&now)
    .bind(&now)
    .execute(&mut *tx)
    .await?;
    sqlx::query(
        "INSERT INTO thread_reads (thread_id, account_id, last_read_index)
         VALUES (?, ?, 0)
         ON CONFLICT(thread_id, account_id) DO UPDATE SET last_read_index = 0",
    )
    .bind(&thread_id)
    .bind(actor_id)
    .execute(&mut *tx)
    .await?;
    let channel_slug: String = sqlx::query_scalar("SELECT slug FROM channels WHERE id = ?")
        .bind(channel_id)
        .fetch_one(&mut *tx)
        .await?;
    upsert_search_index_tx(
        &mut tx,
        SearchIndexInput {
            kind: "thread",
            object_id: &thread_id,
            channel_id: Some(channel_id),
            thread_id: Some(&thread_id),
            conversation_id: None,
            title,
            body,
            context: &format!("#{channel_slug}"),
        },
    )
    .await?;
    create_mention_notifications_tx(
        &mut tx,
        actor_id,
        MentionInput {
            source_kind: "thread",
            source_id: &thread_id,
            channel_id: Some(channel_id),
            thread_id: Some(&thread_id),
            conversation_id: None,
            obj_index: None,
            title,
            body,
        },
    )
    .await?;
    let event = insert_event(
        &mut tx,
        Some(channel_id),
        Some(&thread_id),
        None,
        "thread.created",
        serde_json::json!({"thread_id": thread_id, "channel_id": channel_id, "title": title}),
    )
    .await?;
    tx.commit().await?;
    publish(live_tx, event);
    Ok(thread_id)
}

async fn add_comment(
    pool: &SqlitePool,
    live_tx: &broadcast::Sender<LiveEvent>,
    actor_id: &str,
    thread_id: &str,
    body: &str,
) -> anyhow::Result<()> {
    anyhow::ensure!(!body.trim().is_empty(), "Comment body is required");
    let mut tx = begin(pool).await?;
    let row = sqlx::query(
        "SELECT channel_id, last_comment_index FROM threads WHERE id = ? AND deleted_at IS NULL",
    )
    .bind(thread_id)
    .fetch_optional(&mut *tx)
    .await?;
    let Some(row) = row else {
        bail!("Thread not found");
    };
    let channel_id: String = row.get("channel_id");
    let current_index: i64 = row.get("last_comment_index");
    ensure_can_view_channel(&mut tx, actor_id, &channel_id).await?;
    let next_index = current_index + 1;
    let now = now();
    let comment_id = id();
    sqlx::query(
        "INSERT INTO comments
         (id, thread_id, channel_id, author_account_id, obj_index, body, created_at, updated_at)
         VALUES (?, ?, ?, ?, ?, ?, ?, ?)",
    )
    .bind(&comment_id)
    .bind(thread_id)
    .bind(&channel_id)
    .bind(actor_id)
    .bind(next_index)
    .bind(body.trim())
    .bind(&now)
    .bind(&now)
    .execute(&mut *tx)
    .await?;
    sqlx::query(
        "UPDATE threads
         SET comment_count = comment_count + 1,
             last_comment_index = ?,
             last_activity_at = ?,
             updated_at = ?
         WHERE id = ?",
    )
    .bind(next_index)
    .bind(&now)
    .bind(&now)
    .bind(thread_id)
    .execute(&mut *tx)
    .await?;
    sqlx::query(
        "INSERT INTO thread_reads (thread_id, account_id, last_read_index)
         VALUES (?, ?, ?)
         ON CONFLICT(thread_id, account_id)
         DO UPDATE SET last_read_index = excluded.last_read_index",
    )
    .bind(thread_id)
    .bind(actor_id)
    .bind(next_index)
    .execute(&mut *tx)
    .await?;
    let thread_title: String = sqlx::query_scalar("SELECT title FROM threads WHERE id = ?")
        .bind(thread_id)
        .fetch_one(&mut *tx)
        .await?;
    let channel_slug: String = sqlx::query_scalar("SELECT slug FROM channels WHERE id = ?")
        .bind(&channel_id)
        .fetch_one(&mut *tx)
        .await?;
    upsert_search_index_tx(
        &mut tx,
        SearchIndexInput {
            kind: "comment",
            object_id: &comment_id,
            channel_id: Some(&channel_id),
            thread_id: Some(thread_id),
            conversation_id: None,
            title: &thread_title,
            body: body.trim(),
            context: &format!("#{channel_slug}"),
        },
    )
    .await?;
    create_mention_notifications_tx(
        &mut tx,
        actor_id,
        MentionInput {
            source_kind: "comment",
            source_id: &comment_id,
            channel_id: Some(&channel_id),
            thread_id: Some(thread_id),
            conversation_id: None,
            obj_index: Some(next_index),
            title: &thread_title,
            body: body.trim(),
        },
    )
    .await?;
    create_thread_reply_notifications_tx(
        &mut tx,
        actor_id,
        ReplyNotificationInput {
            thread_id,
            channel_id: &channel_id,
            comment_id: &comment_id,
            obj_index: next_index,
            title: &thread_title,
            body: body.trim(),
        },
    )
    .await?;
    let event = insert_event(
        &mut tx,
        Some(&channel_id),
        Some(thread_id),
        None,
        "comment.created",
        serde_json::json!({"thread_id": thread_id, "channel_id": channel_id, "obj_index": next_index}),
    )
    .await?;
    tx.commit().await?;
    publish(live_tx, event);
    Ok(())
}

async fn open_dm(
    pool: &SqlitePool,
    live_tx: &broadcast::Sender<LiveEvent>,
    actor_id: &str,
    target: &str,
) -> anyhow::Result<String> {
    let target = target.trim().trim_start_matches('@');
    let mut tx = begin(pool).await?;
    let actor = load_account_tx(&mut tx, actor_id).await?;
    anyhow::ensure!(actor.activated, "Account is not activated");
    let target_row = sqlx::query("SELECT id FROM accounts WHERE lower(username) = lower(?) AND activated_at IS NOT NULL AND disabled_at IS NULL")
        .bind(target)
        .fetch_optional(&mut *tx)
        .await?;
    let Some(target_row) = target_row else {
        bail!("User @{target} not found");
    };
    let target_id: String = target_row.get("id");
    anyhow::ensure!(target_id != actor_id, "Cannot DM yourself");
    let dm_key = dm_key(actor_id, &target_id);
    let now = now();
    let conversation_id = if let Some(existing) =
        sqlx::query_scalar::<_, String>("SELECT id FROM conversations WHERE dm_key = ?")
            .bind(&dm_key)
            .fetch_optional(&mut *tx)
            .await?
    {
        existing
    } else {
        let conversation_id = id();
        sqlx::query(
            "INSERT INTO conversations
             (id, dm_key, creator_account_id, last_activity_at, created_at)
             VALUES (?, ?, ?, ?, ?)",
        )
        .bind(&conversation_id)
        .bind(&dm_key)
        .bind(actor_id)
        .bind(&now)
        .bind(&now)
        .execute(&mut *tx)
        .await?;
        for member_id in [actor_id, target_id.as_str()] {
            sqlx::query(
                "INSERT INTO conversation_members (conversation_id, account_id, joined_at)
                 VALUES (?, ?, ?)",
            )
            .bind(&conversation_id)
            .bind(member_id)
            .bind(&now)
            .execute(&mut *tx)
            .await?;
        }
        conversation_id
    };
    let event = insert_event(
        &mut tx,
        None,
        None,
        Some(&conversation_id),
        "conversation.opened",
        serde_json::json!({"conversation_id": conversation_id}),
    )
    .await?;
    tx.commit().await?;
    publish(live_tx, event);
    Ok(conversation_id)
}

async fn send_dm(
    pool: &SqlitePool,
    live_tx: &broadcast::Sender<LiveEvent>,
    actor_id: &str,
    conversation_id: &str,
    body: &str,
) -> anyhow::Result<()> {
    anyhow::ensure!(!body.trim().is_empty(), "Message body is required");
    let mut tx = begin(pool).await?;
    let is_member: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM conversation_members WHERE conversation_id = ? AND account_id = ?",
    )
    .bind(conversation_id)
    .bind(actor_id)
    .fetch_one(&mut *tx)
    .await?;
    anyhow::ensure!(is_member > 0, "Not a participant in this conversation");
    let current_index: i64 =
        sqlx::query_scalar("SELECT last_message_index FROM conversations WHERE id = ?")
            .bind(conversation_id)
            .fetch_one(&mut *tx)
            .await?;
    let next_index = current_index + 1;
    let now = now();
    let message_id = id();
    sqlx::query(
        "INSERT INTO conversation_messages
         (id, conversation_id, author_account_id, obj_index, body, created_at, updated_at)
         VALUES (?, ?, ?, ?, ?, ?, ?)",
    )
    .bind(&message_id)
    .bind(conversation_id)
    .bind(actor_id)
    .bind(next_index)
    .bind(body.trim())
    .bind(&now)
    .bind(&now)
    .execute(&mut *tx)
    .await?;
    sqlx::query(
        "UPDATE conversations SET last_message_index = ?, last_activity_at = ? WHERE id = ?",
    )
    .bind(next_index)
    .bind(&now)
    .bind(conversation_id)
    .execute(&mut *tx)
    .await?;
    sqlx::query(
        "UPDATE conversation_members SET last_read_index = ? WHERE conversation_id = ? AND account_id = ?",
    )
    .bind(next_index)
    .bind(conversation_id)
    .bind(actor_id)
    .execute(&mut *tx)
    .await?;
    upsert_search_index_tx(
        &mut tx,
        SearchIndexInput {
            kind: "dm",
            object_id: &message_id,
            channel_id: None,
            thread_id: None,
            conversation_id: Some(conversation_id),
            title: "DM",
            body: body.trim(),
            context: "DM",
        },
    )
    .await?;
    create_dm_notifications_tx(
        &mut tx,
        actor_id,
        conversation_id,
        &message_id,
        next_index,
        body.trim(),
    )
    .await?;
    create_mention_notifications_tx(
        &mut tx,
        actor_id,
        MentionInput {
            source_kind: "dm",
            source_id: &message_id,
            channel_id: None,
            thread_id: None,
            conversation_id: Some(conversation_id),
            obj_index: Some(next_index),
            title: "DM",
            body: body.trim(),
        },
    )
    .await?;
    let event = insert_event(
        &mut tx,
        None,
        None,
        Some(conversation_id),
        "conversation.message_created",
        serde_json::json!({"conversation_id": conversation_id, "obj_index": next_index}),
    )
    .await?;
    tx.commit().await?;
    publish(live_tx, event);
    Ok(())
}

async fn ensure_channel_name_available(
    tx: &mut Transaction<'_, Sqlite>,
    slug: &str,
) -> anyhow::Result<()> {
    let existing_channel: Option<String> =
        sqlx::query_scalar("SELECT id FROM channels WHERE slug = ? AND archived_at IS NULL")
            .bind(slug)
            .fetch_optional(&mut **tx)
            .await?;
    anyhow::ensure!(
        existing_channel.is_none(),
        "A channel or thread named '{slug}' already exists"
    );
    anyhow::ensure!(
        !active_thread_name_exists(tx, None, slug).await?,
        "A channel or thread named '{slug}' already exists"
    );
    Ok(())
}

async fn ensure_thread_name_available(
    tx: &mut Transaction<'_, Sqlite>,
    channel_id: &str,
    title_key: &str,
) -> anyhow::Result<()> {
    let existing_channel: Option<String> =
        sqlx::query_scalar("SELECT id FROM channels WHERE slug = ? AND archived_at IS NULL")
            .bind(title_key)
            .fetch_optional(&mut **tx)
            .await?;
    anyhow::ensure!(
        existing_channel.is_none(),
        "A channel or thread named '{title_key}' already exists"
    );
    anyhow::ensure!(
        !active_thread_name_exists(tx, Some(channel_id), title_key).await?,
        "A thread named '{title_key}' already exists in this channel"
    );
    Ok(())
}

async fn active_thread_name_exists(
    tx: &mut Transaction<'_, Sqlite>,
    channel_id: Option<&str>,
    name_key: &str,
) -> anyhow::Result<bool> {
    let rows = if let Some(channel_id) = channel_id {
        sqlx::query_scalar::<_, String>(
            "SELECT title
             FROM threads
             WHERE channel_id = ?
               AND deleted_at IS NULL
               AND archived_at IS NULL",
        )
        .bind(channel_id)
        .fetch_all(&mut **tx)
        .await?
    } else {
        sqlx::query_scalar::<_, String>(
            "SELECT title
             FROM threads
             WHERE deleted_at IS NULL
               AND archived_at IS NULL",
        )
        .fetch_all(&mut **tx)
        .await?
    };
    Ok(rows
        .into_iter()
        .any(|title| normalize_name_key(&title) == name_key))
}

#[derive(Clone, Debug)]
struct ChannelMeta {
    id: String,
    slug: String,
    visibility: String,
    created_by_account_id: String,
}

#[derive(Clone, Debug)]
struct ThreadMeta {
    channel_id: String,
    creator_account_id: String,
    title: String,
    body: String,
}

async fn require_admin_tx(
    tx: &mut Transaction<'_, Sqlite>,
    actor_id: &str,
) -> anyhow::Result<Account> {
    let actor = load_account_tx(tx, actor_id).await?;
    anyhow::ensure!(
        actor.activated && actor.role.can_admin(),
        "Only owners/admins can perform this action"
    );
    Ok(actor)
}

fn ensure_can_manage_account(actor: &Account, target: &Account) -> anyhow::Result<()> {
    anyhow::ensure!(
        actor.role.can_admin(),
        "Only owners/admins can manage users"
    );
    if actor.role != Role::Owner && target.role == Role::Owner {
        bail!("Only owners can manage owner accounts");
    }
    Ok(())
}

async fn ensure_not_last_active_owner(
    tx: &mut Transaction<'_, Sqlite>,
    target_id: &str,
) -> anyhow::Result<()> {
    let count: i64 = sqlx::query_scalar(
        "SELECT COUNT(*)
         FROM accounts
         WHERE id <> ?
           AND role = 'owner'
           AND activated_at IS NOT NULL
           AND disabled_at IS NULL",
    )
    .bind(target_id)
    .fetch_one(&mut **tx)
    .await?;
    anyhow::ensure!(count > 0, "Cannot remove the last active owner");
    Ok(())
}

async fn ensure_owner_keeps_active_key(
    tx: &mut Transaction<'_, Sqlite>,
    account_id: &str,
) -> anyhow::Result<()> {
    let active_keys: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM ssh_keys WHERE account_id = ? AND revoked_at IS NULL",
    )
    .bind(account_id)
    .fetch_one(&mut **tx)
    .await?;
    if active_keys <= 1 {
        ensure_not_last_active_owner(tx, account_id).await?;
    }
    Ok(())
}

async fn load_account_by_username_tx(
    tx: &mut Transaction<'_, Sqlite>,
    username: &str,
) -> anyhow::Result<Account> {
    let row = sqlx::query(
        "SELECT id, username, display_name, role, activated_at
         FROM accounts
         WHERE lower(username) = lower(?)",
    )
    .bind(username.trim().trim_start_matches('@'))
    .fetch_optional(&mut **tx)
    .await?;
    let Some(row) = row else {
        bail!("User not found");
    };
    Ok(account_from_row(row))
}

async fn load_channel_by_slug_tx(
    tx: &mut Transaction<'_, Sqlite>,
    slug: &str,
) -> anyhow::Result<ChannelMeta> {
    let slug = slug.trim().trim_start_matches('#').to_lowercase();
    let row = sqlx::query(
        "SELECT id, slug, visibility, created_by_account_id
         FROM channels
         WHERE slug = ? AND archived_at IS NULL",
    )
    .bind(&slug)
    .fetch_optional(&mut **tx)
    .await?;
    let Some(row) = row else {
        bail!("Channel #{slug} not found");
    };
    Ok(ChannelMeta {
        id: row.get("id"),
        slug: row.get("slug"),
        visibility: row.get("visibility"),
        created_by_account_id: row.get("created_by_account_id"),
    })
}

async fn load_channel_by_slug_any_tx(
    tx: &mut Transaction<'_, Sqlite>,
    slug: &str,
) -> anyhow::Result<ChannelMeta> {
    let slug = slug.trim().trim_start_matches('#').to_lowercase();
    let row = sqlx::query(
        "SELECT id, slug, visibility, created_by_account_id
         FROM channels
         WHERE slug = ?",
    )
    .bind(&slug)
    .fetch_optional(&mut **tx)
    .await?;
    let Some(row) = row else {
        bail!("Channel #{slug} not found");
    };
    Ok(ChannelMeta {
        id: row.get("id"),
        slug: row.get("slug"),
        visibility: row.get("visibility"),
        created_by_account_id: row.get("created_by_account_id"),
    })
}

async fn load_thread_meta_tx(
    tx: &mut Transaction<'_, Sqlite>,
    thread_id: &str,
) -> anyhow::Result<ThreadMeta> {
    let row = sqlx::query(
        "SELECT id, channel_id, creator_account_id, title, body
         FROM threads
         WHERE id = ? AND deleted_at IS NULL",
    )
    .bind(thread_id)
    .fetch_optional(&mut **tx)
    .await?;
    let Some(row) = row else {
        bail!("Thread not found");
    };
    Ok(ThreadMeta {
        channel_id: row.get("channel_id"),
        creator_account_id: row.get("creator_account_id"),
        title: row.get("title"),
        body: row.get("body"),
    })
}

async fn ensure_can_manage_channel(
    tx: &mut Transaction<'_, Sqlite>,
    actor_id: &str,
    channel: &ChannelMeta,
) -> anyhow::Result<Account> {
    let actor = load_account_tx(tx, actor_id).await?;
    anyhow::ensure!(actor.activated, "Account is not activated");
    if actor.role.can_admin() || channel.created_by_account_id == actor_id {
        return Ok(actor);
    }
    bail!("You do not manage this channel")
}

async fn ensure_can_modify_thread(
    tx: &mut Transaction<'_, Sqlite>,
    actor_id: &str,
    thread: &ThreadMeta,
    require_moderator: bool,
) -> anyhow::Result<Account> {
    let actor = load_account_tx(tx, actor_id).await?;
    anyhow::ensure!(actor.activated, "Account is not activated");
    let channel = load_channel_by_id_tx(tx, &thread.channel_id).await?;
    if actor.role.can_admin() || channel.created_by_account_id == actor_id {
        return Ok(actor);
    }
    if !require_moderator && thread.creator_account_id == actor_id {
        return Ok(actor);
    }
    bail!("You cannot modify this thread")
}

async fn load_channel_by_id_tx(
    tx: &mut Transaction<'_, Sqlite>,
    channel_id: &str,
) -> anyhow::Result<ChannelMeta> {
    let row = sqlx::query(
        "SELECT id, slug, visibility, created_by_account_id
         FROM channels
         WHERE id = ? AND archived_at IS NULL",
    )
    .bind(channel_id)
    .fetch_optional(&mut **tx)
    .await?;
    let Some(row) = row else {
        bail!("Channel not found");
    };
    Ok(ChannelMeta {
        id: row.get("id"),
        slug: row.get("slug"),
        visibility: row.get("visibility"),
        created_by_account_id: row.get("created_by_account_id"),
    })
}

async fn update_channel_member(
    pool: &SqlitePool,
    live_tx: &broadcast::Sender<LiveEvent>,
    actor_id: &str,
    slug: &str,
    username: &str,
    add: bool,
) -> anyhow::Result<()> {
    let mut tx = begin(pool).await?;
    let channel = load_channel_by_slug_tx(&mut tx, slug).await?;
    ensure_can_manage_channel(&mut tx, actor_id, &channel).await?;
    anyhow::ensure!(
        channel.visibility == "private",
        "Channel membership is only managed for private channels"
    );
    let target = load_account_by_username_tx(&mut tx, username).await?;
    let now = now();
    if add {
        sqlx::query(
            "INSERT INTO channel_members (channel_id, account_id, role, joined_at)
             VALUES (?, ?, 'member', ?)
             ON CONFLICT(channel_id, account_id) DO NOTHING",
        )
        .bind(&channel.id)
        .bind(&target.id)
        .bind(&now)
        .execute(&mut *tx)
        .await?;
    } else {
        anyhow::ensure!(
            target.id != channel.created_by_account_id,
            "Cannot remove the channel creator"
        );
        sqlx::query("DELETE FROM channel_members WHERE channel_id = ? AND account_id = ?")
            .bind(&channel.id)
            .bind(&target.id)
            .execute(&mut *tx)
            .await?;
    }
    let action = if add {
        "channel.member_added"
    } else {
        "channel.member_removed"
    };
    insert_audit(
        &mut tx,
        Some(actor_id),
        action,
        Some(&channel.id),
        serde_json::json!({"channel": channel.slug, "username": target.username}),
    )
    .await?;
    let event = insert_event(
        &mut tx,
        Some(&channel.id),
        None,
        None,
        action,
        serde_json::json!({"channel_id": channel.id, "account_id": target.id}),
    )
    .await?;
    tx.commit().await?;
    publish(live_tx, event);
    Ok(())
}

#[derive(Clone, Copy)]
enum ThreadFlag {
    Archived,
    Pinned,
    Deleted,
}

async fn update_thread_flag(
    pool: &SqlitePool,
    live_tx: &broadcast::Sender<LiveEvent>,
    actor_id: &str,
    thread_id: &str,
    flag: ThreadFlag,
    enabled: bool,
) -> anyhow::Result<()> {
    let mut tx = begin(pool).await?;
    let thread = load_thread_meta_tx(&mut tx, thread_id).await?;
    ensure_can_modify_thread(
        &mut tx,
        actor_id,
        &thread,
        matches!(flag, ThreadFlag::Pinned),
    )
    .await?;
    let now = now();
    let value = enabled.then_some(now.as_str());
    let (column, action) = match flag {
        ThreadFlag::Archived => (
            "archived_at",
            if enabled {
                "thread.archived"
            } else {
                "thread.unarchived"
            },
        ),
        ThreadFlag::Pinned => (
            "pinned_at",
            if enabled {
                "thread.pinned"
            } else {
                "thread.unpinned"
            },
        ),
        ThreadFlag::Deleted => ("deleted_at", "thread.deleted"),
    };
    let sql = format!("UPDATE threads SET {column} = ?, updated_at = ? WHERE id = ?");
    sqlx::query(&sql)
        .bind(value)
        .bind(&now)
        .bind(thread_id)
        .execute(&mut *tx)
        .await?;
    if matches!(flag, ThreadFlag::Deleted) && enabled {
        delete_search_index_tx(&mut tx, "thread", thread_id).await?;
    }
    insert_audit(
        &mut tx,
        Some(actor_id),
        action,
        Some(thread_id),
        serde_json::json!({"channel_id": thread.channel_id}),
    )
    .await?;
    let event = insert_event(
        &mut tx,
        Some(&thread.channel_id),
        Some(thread_id),
        None,
        action,
        serde_json::json!({"thread_id": thread_id}),
    )
    .await?;
    tx.commit().await?;
    publish(live_tx, event);
    Ok(())
}

async fn upsert_thread_read_state(
    tx: &mut Transaction<'_, Sqlite>,
    account_id: &str,
    thread_id: &str,
    update_mute: bool,
    muted_until: Option<&str>,
    update_saved: bool,
    saved_at: Option<&str>,
) -> anyhow::Result<()> {
    let existing: Option<(Option<String>, Option<String>)> = sqlx::query_as(
        "SELECT muted_until, saved_at FROM thread_reads WHERE thread_id = ? AND account_id = ?",
    )
    .bind(thread_id)
    .bind(account_id)
    .fetch_optional(&mut **tx)
    .await?;
    let next_muted_until = if update_mute {
        muted_until.map(ToOwned::to_owned)
    } else {
        existing.as_ref().and_then(|(value, _)| value.clone())
    };
    let next_saved_at = if update_saved {
        saved_at.map(ToOwned::to_owned)
    } else {
        existing.as_ref().and_then(|(_, value)| value.clone())
    };
    sqlx::query(
        "INSERT INTO thread_reads (thread_id, account_id, muted_until, saved_at)
         VALUES (?, ?, ?, ?)
         ON CONFLICT(thread_id, account_id)
         DO UPDATE SET muted_until = ?, saved_at = ?",
    )
    .bind(thread_id)
    .bind(account_id)
    .bind(next_muted_until.as_deref())
    .bind(next_saved_at.as_deref())
    .bind(next_muted_until.as_deref())
    .bind(next_saved_at.as_deref())
    .execute(&mut **tx)
    .await?;
    Ok(())
}

struct CommentMeta {
    id: String,
    author_account_id: String,
}

async fn load_comment_meta_tx(
    tx: &mut Transaction<'_, Sqlite>,
    thread_id: &str,
    obj_index: i64,
) -> anyhow::Result<CommentMeta> {
    let row = sqlx::query(
        "SELECT id, author_account_id
         FROM comments
         WHERE thread_id = ? AND obj_index = ? AND deleted_at IS NULL",
    )
    .bind(thread_id)
    .bind(obj_index)
    .fetch_optional(&mut **tx)
    .await?;
    let Some(row) = row else {
        bail!("Comment #{obj_index} not found");
    };
    Ok(CommentMeta {
        id: row.get("id"),
        author_account_id: row.get("author_account_id"),
    })
}

async fn update_comment_body(
    pool: &SqlitePool,
    live_tx: &broadcast::Sender<LiveEvent>,
    actor_id: &str,
    thread_id: &str,
    obj_index: i64,
    body: &str,
) -> anyhow::Result<()> {
    let body = body.trim();
    anyhow::ensure!(!body.is_empty(), "Comment body is required");
    let mut tx = begin(pool).await?;
    let thread = load_thread_meta_tx(&mut tx, thread_id).await?;
    ensure_can_view_channel(&mut tx, actor_id, &thread.channel_id).await?;
    let row = load_comment_meta_tx(&mut tx, thread_id, obj_index).await?;
    let actor = load_account_tx(&mut tx, actor_id).await?;
    let channel = load_channel_by_id_tx(&mut tx, &thread.channel_id).await?;
    let can_moderate = actor.role.can_admin() || channel.created_by_account_id == actor_id;
    anyhow::ensure!(
        can_moderate || row.author_account_id == actor_id,
        "You can only edit your own comments"
    );
    let now = now();
    sqlx::query("UPDATE comments SET body = ?, updated_at = ?, edited_at = ? WHERE id = ?")
        .bind(body)
        .bind(&now)
        .bind(&now)
        .bind(&row.id)
        .execute(&mut *tx)
        .await?;
    upsert_search_index_tx(
        &mut tx,
        SearchIndexInput {
            kind: "comment",
            object_id: &row.id,
            channel_id: Some(&thread.channel_id),
            thread_id: Some(thread_id),
            conversation_id: None,
            title: &thread.title,
            body,
            context: &format!("#{}", channel.slug),
        },
    )
    .await?;
    insert_audit(
        &mut tx,
        Some(actor_id),
        "comment.edited",
        Some(&row.id),
        serde_json::json!({"thread_id": thread_id}),
    )
    .await?;
    let event = insert_event(
        &mut tx,
        Some(&thread.channel_id),
        Some(thread_id),
        None,
        "comment.edited",
        serde_json::json!({"thread_id": thread_id, "obj_index": obj_index}),
    )
    .await?;
    tx.commit().await?;
    publish(live_tx, event);
    Ok(())
}

async fn soft_delete_comment(
    pool: &SqlitePool,
    live_tx: &broadcast::Sender<LiveEvent>,
    actor_id: &str,
    thread_id: &str,
    obj_index: i64,
) -> anyhow::Result<()> {
    let mut tx = begin(pool).await?;
    let thread = load_thread_meta_tx(&mut tx, thread_id).await?;
    ensure_can_view_channel(&mut tx, actor_id, &thread.channel_id).await?;
    let row = load_comment_meta_tx(&mut tx, thread_id, obj_index).await?;
    let actor = load_account_tx(&mut tx, actor_id).await?;
    let channel = load_channel_by_id_tx(&mut tx, &thread.channel_id).await?;
    let can_moderate = actor.role.can_admin() || channel.created_by_account_id == actor_id;
    anyhow::ensure!(
        can_moderate || row.author_account_id == actor_id,
        "You can only delete your own comments"
    );
    let now = now();
    sqlx::query("UPDATE comments SET deleted_at = ?, updated_at = ? WHERE id = ?")
        .bind(&now)
        .bind(&now)
        .bind(&row.id)
        .execute(&mut *tx)
        .await?;
    sqlx::query(
        "UPDATE threads SET comment_count = MAX(comment_count - 1, 0), updated_at = ? WHERE id = ?",
    )
    .bind(&now)
    .bind(thread_id)
    .execute(&mut *tx)
    .await?;
    delete_search_index_tx(&mut tx, "comment", &row.id).await?;
    insert_audit(
        &mut tx,
        Some(actor_id),
        "comment.deleted",
        Some(&row.id),
        serde_json::json!({"thread_id": thread_id}),
    )
    .await?;
    let event = insert_event(
        &mut tx,
        Some(&thread.channel_id),
        Some(thread_id),
        None,
        "comment.deleted",
        serde_json::json!({"thread_id": thread_id, "obj_index": obj_index}),
    )
    .await?;
    tx.commit().await?;
    publish(live_tx, event);
    Ok(())
}

struct DmMessageMeta {
    id: String,
    author_account_id: String,
}

async fn load_dm_message_meta_tx(
    tx: &mut Transaction<'_, Sqlite>,
    actor_id: &str,
    conversation_id: &str,
    obj_index: i64,
) -> anyhow::Result<DmMessageMeta> {
    let is_member: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM conversation_members WHERE conversation_id = ? AND account_id = ?",
    )
    .bind(conversation_id)
    .bind(actor_id)
    .fetch_one(&mut **tx)
    .await?;
    anyhow::ensure!(is_member > 0, "Not a participant in this conversation");
    let row = sqlx::query(
        "SELECT id, author_account_id
         FROM conversation_messages
         WHERE conversation_id = ? AND obj_index = ? AND deleted_at IS NULL",
    )
    .bind(conversation_id)
    .bind(obj_index)
    .fetch_optional(&mut **tx)
    .await?;
    let Some(row) = row else {
        bail!("DM message #{obj_index} not found");
    };
    Ok(DmMessageMeta {
        id: row.get("id"),
        author_account_id: row.get("author_account_id"),
    })
}

async fn update_dm_body(
    pool: &SqlitePool,
    live_tx: &broadcast::Sender<LiveEvent>,
    actor_id: &str,
    conversation_id: &str,
    obj_index: i64,
    body: &str,
) -> anyhow::Result<()> {
    let body = body.trim();
    anyhow::ensure!(!body.is_empty(), "Message body is required");
    let mut tx = begin(pool).await?;
    let row = load_dm_message_meta_tx(&mut tx, actor_id, conversation_id, obj_index).await?;
    anyhow::ensure!(
        row.author_account_id == actor_id,
        "You can only edit your own DMs"
    );
    let now = now();
    sqlx::query(
        "UPDATE conversation_messages SET body = ?, updated_at = ?, edited_at = ? WHERE id = ?",
    )
    .bind(body)
    .bind(&now)
    .bind(&now)
    .bind(&row.id)
    .execute(&mut *tx)
    .await?;
    upsert_search_index_tx(
        &mut tx,
        SearchIndexInput {
            kind: "dm",
            object_id: &row.id,
            channel_id: None,
            thread_id: None,
            conversation_id: Some(conversation_id),
            title: "DM",
            body,
            context: "DM",
        },
    )
    .await?;
    insert_audit(
        &mut tx,
        Some(actor_id),
        "dm.edited",
        Some(&row.id),
        serde_json::json!({"conversation_id": conversation_id}),
    )
    .await?;
    let event = insert_event(
        &mut tx,
        None,
        None,
        Some(conversation_id),
        "conversation.message_edited",
        serde_json::json!({"conversation_id": conversation_id, "obj_index": obj_index}),
    )
    .await?;
    tx.commit().await?;
    publish(live_tx, event);
    Ok(())
}

async fn soft_delete_dm(
    pool: &SqlitePool,
    live_tx: &broadcast::Sender<LiveEvent>,
    actor_id: &str,
    conversation_id: &str,
    obj_index: i64,
) -> anyhow::Result<()> {
    let mut tx = begin(pool).await?;
    let row = load_dm_message_meta_tx(&mut tx, actor_id, conversation_id, obj_index).await?;
    anyhow::ensure!(
        row.author_account_id == actor_id,
        "You can only delete your own DMs"
    );
    let now = now();
    sqlx::query("UPDATE conversation_messages SET deleted_at = ?, updated_at = ? WHERE id = ?")
        .bind(&now)
        .bind(&now)
        .bind(&row.id)
        .execute(&mut *tx)
        .await?;
    delete_search_index_tx(&mut tx, "dm", &row.id).await?;
    insert_audit(
        &mut tx,
        Some(actor_id),
        "dm.deleted",
        Some(&row.id),
        serde_json::json!({"conversation_id": conversation_id}),
    )
    .await?;
    let event = insert_event(
        &mut tx,
        None,
        None,
        Some(conversation_id),
        "conversation.message_deleted",
        serde_json::json!({"conversation_id": conversation_id, "obj_index": obj_index}),
    )
    .await?;
    tx.commit().await?;
    publish(live_tx, event);
    Ok(())
}

async fn begin(pool: &SqlitePool) -> anyhow::Result<Transaction<'_, Sqlite>> {
    let tx = pool.begin().await?;
    Ok(tx)
}

async fn load_user_presence(
    pool: &SqlitePool,
    active_account_ids: &HashSet<String>,
) -> anyhow::Result<Vec<UserPresence>> {
    let rows = sqlx::query(
        "SELECT id, username, display_name, last_seen_at
         FROM accounts
         WHERE activated_at IS NOT NULL AND disabled_at IS NULL
         ORDER BY username",
    )
    .fetch_all(pool)
    .await?;
    Ok(rows
        .into_iter()
        .map(|row| {
            let account_id: String = row.get("id");
            UserPresence {
                connected: active_account_ids.contains(&account_id),
                username: row.get("username"),
                display_name: row.get("display_name"),
                last_seen_at: row.get("last_seen_at"),
            }
        })
        .collect())
}

async fn load_notifications(
    pool: &SqlitePool,
    account_id: &str,
    limit: i64,
) -> anyhow::Result<Vec<NotificationSummary>> {
    let limit = limit.clamp(1, 200);
    let rows = sqlx::query(
        "SELECT n.id, n.kind, actor.username AS actor_username, n.title, n.body,
                n.created_at, n.read_at
         FROM notifications n
         LEFT JOIN accounts actor ON actor.id = n.actor_account_id
         WHERE n.account_id = ?
         ORDER BY n.created_at DESC
         LIMIT ?",
    )
    .bind(account_id)
    .bind(limit)
    .fetch_all(pool)
    .await?;
    Ok(rows
        .into_iter()
        .map(|row| NotificationSummary {
            id: row.get("id"),
            kind: row.get("kind"),
            actor_username: row.get("actor_username"),
            title: row.get("title"),
            body: row.get("body"),
            created_at: row.get("created_at"),
            read_at: row.get("read_at"),
        })
        .collect())
}

async fn load_channels(pool: &SqlitePool, account_id: &str) -> anyhow::Result<Vec<Channel>> {
    let current_time = now();
    let rows = sqlx::query(
        "SELECT c.id, c.slug, c.name, c.visibility, c.topic,
                COALESCE(SUM(
                    CASE
                      WHEN t.id IS NULL THEN 0
                      WHEN r.muted_until IS NOT NULL AND r.muted_until > ? THEN 0
                      ELSE (
                        SELECT COUNT(*)
                        FROM comments cm
                        WHERE cm.thread_id = t.id
                          AND cm.deleted_at IS NULL
                          AND cm.obj_index > COALESCE(r.last_read_index, 0)
                      )
                    END
                ), 0) AS unread_count
         FROM channels c
         LEFT JOIN threads t ON t.channel_id = c.id AND t.deleted_at IS NULL AND t.archived_at IS NULL
         LEFT JOIN thread_reads r ON r.thread_id = t.id AND r.account_id = ?
         WHERE c.archived_at IS NULL
           AND EXISTS (
             SELECT 1 FROM channel_members m
             WHERE m.channel_id = c.id AND m.account_id = ?
           )
         GROUP BY c.id, c.slug, c.name, c.visibility, c.topic
         ORDER BY CASE WHEN c.slug = 'general' THEN 0 ELSE 1 END, c.slug",
    )
    .bind(current_time)
    .bind(account_id)
    .bind(account_id)
    .fetch_all(pool)
    .await?;
    Ok(rows
        .into_iter()
        .map(|row| Channel {
            id: row.get("id"),
            slug: row.get("slug"),
            name: row.get("name"),
            visibility: row.get("visibility"),
            topic: row.get("topic"),
            unread_count: row.get("unread_count"),
        })
        .collect())
}

async fn load_threads(
    pool: &SqlitePool,
    account_id: &str,
    channel_id: &str,
) -> anyhow::Result<Vec<ThreadItem>> {
    let rows = sqlx::query(
        "SELECT t.id, t.channel_id, t.title, t.body, a.username AS author,
                t.comment_count, t.last_comment_index,
                CASE
                  WHEN r.muted_until IS NOT NULL AND r.muted_until > ? THEN 0
                  ELSE (
                    SELECT COUNT(*)
                    FROM comments cm
                    WHERE cm.thread_id = t.id
                      AND cm.deleted_at IS NULL
                      AND cm.obj_index > COALESCE(r.last_read_index, 0)
                  )
                END AS unread_count,
                t.last_activity_at, t.created_at, t.edited_at, t.archived_at, t.pinned_at,
                r.muted_until, r.saved_at,
                COALESCE((
                  SELECT group_concat(emoji || ' ' || count, ' ')
                  FROM (
                    SELECT emoji, COUNT(*) AS count
                    FROM reactions
                    WHERE source_kind = 'thread' AND source_id = t.id
                    GROUP BY emoji
                    ORDER BY emoji
                  )
                ), '') AS reactions
         FROM threads t
         JOIN accounts a ON a.id = t.creator_account_id
         LEFT JOIN thread_reads r ON r.thread_id = t.id AND r.account_id = ?
         WHERE t.channel_id = ? AND t.deleted_at IS NULL
         ORDER BY t.pinned_at IS NULL, t.pinned_at DESC, t.last_activity_at DESC, t.id DESC
         LIMIT 200",
    )
    .bind(now())
    .bind(account_id)
    .bind(channel_id)
    .fetch_all(pool)
    .await?;
    Ok(rows
        .into_iter()
        .map(|row| ThreadItem {
            id: row.get("id"),
            channel_id: row.get("channel_id"),
            title: row.get("title"),
            body: row.get("body"),
            author: row.get("author"),
            comment_count: row.get("comment_count"),
            last_comment_index: row.get("last_comment_index"),
            unread_count: row.get("unread_count"),
            last_activity_at: row.get("last_activity_at"),
            created_at: row.get("created_at"),
            edited_at: row.get("edited_at"),
            archived_at: row.get("archived_at"),
            pinned_at: row.get("pinned_at"),
            muted_until: row.get("muted_until"),
            saved_at: row.get("saved_at"),
            reactions: row.get("reactions"),
        })
        .collect())
}

async fn load_comments(
    pool: &SqlitePool,
    thread_id: &str,
    limit: i64,
) -> anyhow::Result<(Vec<CommentItem>, bool)> {
    let limit = limit.clamp(1, MAX_HISTORY_LIMIT);
    let rows = sqlx::query(
        "SELECT id, author, obj_index, body, created_at, edited_at, reactions
         FROM (
           SELECT c.id, a.username AS author, c.obj_index, c.body, c.created_at, c.edited_at,
                  COALESCE((
                    SELECT group_concat(emoji || ' ' || count, ' ')
                    FROM (
                      SELECT emoji, COUNT(*) AS count
                      FROM reactions
                      WHERE source_kind = 'comment' AND source_id = c.id
                      GROUP BY emoji
                      ORDER BY emoji
                    )
                  ), '') AS reactions
           FROM comments c
           JOIN accounts a ON a.id = c.author_account_id
           WHERE c.thread_id = ? AND c.deleted_at IS NULL
           ORDER BY c.obj_index DESC
           LIMIT ?
         ) recent
         ORDER BY obj_index ASC",
    )
    .bind(thread_id)
    .bind(limit.saturating_add(1))
    .fetch_all(pool)
    .await?;
    let mut comments: Vec<_> = rows
        .into_iter()
        .map(|row| CommentItem {
            id: row.get("id"),
            author: row.get("author"),
            obj_index: row.get("obj_index"),
            body: row.get("body"),
            created_at: row.get("created_at"),
            edited_at: row.get("edited_at"),
            reactions: row.get("reactions"),
        })
        .collect();
    let has_more = comments.len() > limit as usize;
    if has_more {
        comments.remove(0);
    }
    Ok((comments, has_more))
}

async fn load_conversations(
    pool: &SqlitePool,
    account_id: &str,
) -> anyhow::Result<Vec<Conversation>> {
    let rows = sqlx::query(
        "SELECT c.id,
                peer.username AS peer_username,
                c.last_message_index,
                CASE
                  WHEN me.muted_until IS NOT NULL AND me.muted_until > ? THEN 0
                  ELSE (
                    SELECT COUNT(*)
                    FROM conversation_messages msg
                    WHERE msg.conversation_id = c.id
                      AND msg.deleted_at IS NULL
                      AND msg.obj_index > me.last_read_index
                  )
                END AS unread_count,
                c.last_activity_at,
                me.muted_until,
                me.saved_at,
                (
                    SELECT body
                    FROM conversation_messages latest
                    WHERE latest.conversation_id = c.id AND latest.deleted_at IS NULL
                    ORDER BY latest.obj_index DESC
                    LIMIT 1
                ) AS last_message_preview
         FROM conversations c
         JOIN conversation_members me ON me.conversation_id = c.id AND me.account_id = ?
         JOIN conversation_members other ON other.conversation_id = c.id AND other.account_id <> ?
         JOIN accounts peer ON peer.id = other.account_id
         WHERE c.archived_at IS NULL
         ORDER BY c.last_activity_at DESC",
    )
    .bind(now())
    .bind(account_id)
    .bind(account_id)
    .fetch_all(pool)
    .await?;
    Ok(rows
        .into_iter()
        .map(|row| Conversation {
            id: row.get("id"),
            peer_username: row.get("peer_username"),
            last_message_index: row.get("last_message_index"),
            unread_count: row.get("unread_count"),
            last_activity_at: row.get("last_activity_at"),
            last_message_preview: row.get("last_message_preview"),
            muted_until: row.get("muted_until"),
            saved_at: row.get("saved_at"),
        })
        .collect())
}

async fn load_conversation_messages(
    pool: &SqlitePool,
    conversation_id: &str,
    limit: i64,
) -> anyhow::Result<(Vec<ConversationMessage>, bool)> {
    let limit = limit.clamp(1, MAX_HISTORY_LIMIT);
    let rows = sqlx::query(
        "SELECT id, author, obj_index, body, created_at, edited_at, reactions
         FROM (
           SELECT m.id, a.username AS author, m.obj_index, m.body, m.created_at, m.edited_at,
                  COALESCE((
                    SELECT group_concat(emoji || ' ' || count, ' ')
                    FROM (
                      SELECT emoji, COUNT(*) AS count
                      FROM reactions
                      WHERE source_kind = 'dm' AND source_id = m.id
                      GROUP BY emoji
                      ORDER BY emoji
                    )
                  ), '') AS reactions
           FROM conversation_messages m
           JOIN accounts a ON a.id = m.author_account_id
           WHERE m.conversation_id = ? AND m.deleted_at IS NULL
           ORDER BY m.obj_index DESC
           LIMIT ?
         ) recent
         ORDER BY obj_index ASC",
    )
    .bind(conversation_id)
    .bind(limit.saturating_add(1))
    .fetch_all(pool)
    .await?;
    let mut messages: Vec<_> = rows
        .into_iter()
        .map(|row| ConversationMessage {
            id: row.get("id"),
            author: row.get("author"),
            obj_index: row.get("obj_index"),
            body: row.get("body"),
            created_at: row.get("created_at"),
            edited_at: row.get("edited_at"),
            reactions: row.get("reactions"),
        })
        .collect();
    let has_more = messages.len() > limit as usize;
    if has_more {
        messages.remove(0);
    }
    Ok((messages, has_more))
}

async fn load_account_tx(
    tx: &mut Transaction<'_, Sqlite>,
    account_id: &str,
) -> anyhow::Result<Account> {
    let row = sqlx::query(
        "SELECT id, username, display_name, role, activated_at
         FROM accounts WHERE id = ? AND disabled_at IS NULL",
    )
    .bind(account_id)
    .fetch_one(&mut **tx)
    .await?;
    Ok(account_from_row(row))
}

async fn ensure_can_view_channel(
    tx: &mut Transaction<'_, Sqlite>,
    account_id: &str,
    channel_id: &str,
) -> anyhow::Result<()> {
    let account = load_account_tx(tx, account_id).await?;
    anyhow::ensure!(account.activated, "Account is not activated");
    let count: i64 = sqlx::query_scalar(
        "SELECT COUNT(*)
         FROM channels c
         WHERE c.id = ?
           AND c.archived_at IS NULL
           AND EXISTS (
             SELECT 1 FROM channel_members m
             WHERE m.channel_id = c.id AND m.account_id = ?
           )",
    )
    .bind(channel_id)
    .bind(account_id)
    .fetch_one(&mut **tx)
    .await?;
    anyhow::ensure!(count > 0, "You do not have access to this channel");
    Ok(())
}

async fn set_reaction_tx(
    tx: &mut Transaction<'_, Sqlite>,
    account_id: &str,
    source_kind: &str,
    source_id: &str,
    emoji: &str,
    remove: bool,
) -> anyhow::Result<()> {
    let emoji = emoji.trim();
    validate_emoji(emoji)?;
    if remove {
        sqlx::query(
            "DELETE FROM reactions
             WHERE source_kind = ? AND source_id = ? AND account_id = ? AND emoji = ?",
        )
        .bind(source_kind)
        .bind(source_id)
        .bind(account_id)
        .bind(emoji)
        .execute(&mut **tx)
        .await?;
    } else {
        sqlx::query(
            "INSERT INTO reactions (id, source_kind, source_id, account_id, emoji, created_at)
             VALUES (?, ?, ?, ?, ?, ?)
             ON CONFLICT(source_kind, source_id, account_id, emoji) DO NOTHING",
        )
        .bind(id())
        .bind(source_kind)
        .bind(source_id)
        .bind(account_id)
        .bind(emoji)
        .bind(now())
        .execute(&mut **tx)
        .await?;
    }
    Ok(())
}

fn validate_emoji(emoji: &str) -> anyhow::Result<()> {
    anyhow::ensure!(!emoji.is_empty(), "Emoji is required");
    anyhow::ensure!(emoji.chars().count() <= 8, "Emoji is too long");
    anyhow::ensure!(
        !emoji
            .chars()
            .any(|ch| ch.is_ascii_alphanumeric() || ch.is_ascii_control()),
        "Use a Unicode emoji reaction"
    );
    Ok(())
}

async fn insert_event(
    tx: &mut Transaction<'_, Sqlite>,
    channel_id: Option<&str>,
    thread_id: Option<&str>,
    conversation_id: Option<&str>,
    kind: &str,
    payload: serde_json::Value,
) -> anyhow::Result<LiveEvent> {
    let now = now();
    let payload_json = serde_json::to_string(&payload)?;
    let result = sqlx::query(
        "INSERT INTO event_log
         (created_at, channel_id, thread_id, conversation_id, kind, payload_json)
         VALUES (?, ?, ?, ?, ?, ?)",
    )
    .bind(&now)
    .bind(channel_id)
    .bind(thread_id)
    .bind(conversation_id)
    .bind(kind)
    .bind(&payload_json)
    .execute(&mut **tx)
    .await?;
    Ok(LiveEvent {
        seq: result.last_insert_rowid(),
        channel_id: channel_id.map(ToOwned::to_owned),
        thread_id: thread_id.map(ToOwned::to_owned),
        conversation_id: conversation_id.map(ToOwned::to_owned),
        kind: kind.to_string(),
        payload,
    })
}

async fn insert_audit(
    tx: &mut Transaction<'_, Sqlite>,
    actor_account_id: Option<&str>,
    action: &str,
    target: Option<&str>,
    metadata: serde_json::Value,
) -> anyhow::Result<()> {
    sqlx::query(
        "INSERT INTO audit_log
         (id, actor_account_id, action, target, metadata_json, created_at)
         VALUES (?, ?, ?, ?, ?, ?)",
    )
    .bind(id())
    .bind(actor_account_id)
    .bind(action)
    .bind(target)
    .bind(serde_json::to_string(&metadata)?)
    .bind(now())
    .execute(&mut **tx)
    .await?;
    Ok(())
}

struct SearchIndexInput<'a> {
    kind: &'a str,
    object_id: &'a str,
    channel_id: Option<&'a str>,
    thread_id: Option<&'a str>,
    conversation_id: Option<&'a str>,
    title: &'a str,
    body: &'a str,
    context: &'a str,
}

async fn upsert_search_index_tx(
    tx: &mut Transaction<'_, Sqlite>,
    input: SearchIndexInput<'_>,
) -> anyhow::Result<()> {
    sqlx::query("DELETE FROM search_index WHERE kind = ? AND object_id = ?")
        .bind(input.kind)
        .bind(input.object_id)
        .execute(&mut **tx)
        .await?;
    sqlx::query(
        "INSERT INTO search_index
         (kind, object_id, channel_id, thread_id, conversation_id, title, body, context)
         VALUES (?, ?, ?, ?, ?, ?, ?, ?)",
    )
    .bind(input.kind)
    .bind(input.object_id)
    .bind(input.channel_id)
    .bind(input.thread_id)
    .bind(input.conversation_id)
    .bind(input.title)
    .bind(input.body)
    .bind(input.context)
    .execute(&mut **tx)
    .await?;
    Ok(())
}

async fn delete_search_index_tx(
    tx: &mut Transaction<'_, Sqlite>,
    kind: &str,
    object_id: &str,
) -> anyhow::Result<()> {
    sqlx::query("DELETE FROM search_index WHERE kind = ? AND object_id = ?")
        .bind(kind)
        .bind(object_id)
        .execute(&mut **tx)
        .await?;
    Ok(())
}

fn fts_query(query: &str) -> String {
    let mut terms = Vec::new();
    let mut current = String::new();
    for ch in query.chars() {
        if ch.is_ascii_alphanumeric() {
            current.push(ch.to_ascii_lowercase());
        } else if !current.is_empty() {
            terms.push(std::mem::take(&mut current));
        }
    }
    if !current.is_empty() {
        terms.push(current);
    }
    if terms.is_empty() {
        query.replace('"', " ")
    } else {
        terms
            .into_iter()
            .map(|term| format!("{term}*"))
            .collect::<Vec<_>>()
            .join(" AND ")
    }
}

#[derive(Clone, Copy)]
struct NotificationInput<'a> {
    kind: &'a str,
    source_kind: &'a str,
    source_id: &'a str,
    channel_id: Option<&'a str>,
    thread_id: Option<&'a str>,
    conversation_id: Option<&'a str>,
    title: &'a str,
    body: &'a str,
}

async fn create_notification_tx(
    tx: &mut Transaction<'_, Sqlite>,
    account_id: &str,
    actor_id: Option<&str>,
    input: NotificationInput<'_>,
) -> anyhow::Result<String> {
    if actor_id == Some(account_id) {
        return Ok(String::new());
    }
    if let Some(thread_id) = input.thread_id {
        let muted: i64 = sqlx::query_scalar(
            "SELECT COUNT(*) FROM thread_reads
             WHERE thread_id = ? AND account_id = ?
               AND muted_until IS NOT NULL AND muted_until > ?",
        )
        .bind(thread_id)
        .bind(account_id)
        .bind(now())
        .fetch_one(&mut **tx)
        .await?;
        if muted > 0 {
            return Ok(String::new());
        }
    }
    if let Some(conversation_id) = input.conversation_id {
        let muted: i64 = sqlx::query_scalar(
            "SELECT COUNT(*) FROM conversation_members
             WHERE conversation_id = ? AND account_id = ?
               AND muted_until IS NOT NULL AND muted_until > ?",
        )
        .bind(conversation_id)
        .bind(account_id)
        .bind(now())
        .fetch_one(&mut **tx)
        .await?;
        if muted > 0 {
            return Ok(String::new());
        }
    }
    let id = id();
    let created_at = now();
    sqlx::query(
        "INSERT INTO notifications
         (id, account_id, actor_account_id, kind, source_kind, source_id, channel_id,
          thread_id, conversation_id, title, body, created_at)
         VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
    )
    .bind(&id)
    .bind(account_id)
    .bind(actor_id)
    .bind(input.kind)
    .bind(input.source_kind)
    .bind(input.source_id)
    .bind(input.channel_id)
    .bind(input.thread_id)
    .bind(input.conversation_id)
    .bind(input.title)
    .bind(input.body)
    .bind(&created_at)
    .execute(&mut **tx)
    .await?;
    queue_webhook_jobs_tx(tx, &id, input.kind, input.title, input.body).await?;
    Ok(id)
}

async fn queue_webhook_jobs_tx(
    tx: &mut Transaction<'_, Sqlite>,
    notification_id: &str,
    kind: &str,
    title: &str,
    body: &str,
) -> anyhow::Result<()> {
    let webhooks = sqlx::query(
        "SELECT id, name, url FROM webhook_subscriptions WHERE enabled = 1 AND disabled_at IS NULL",
    )
    .fetch_all(&mut **tx)
    .await?;
    let now = now();
    for webhook in webhooks {
        let payload = serde_json::json!({
            "notification_id": notification_id,
            "kind": kind,
            "title": title,
            "body": body,
            "webhook": webhook.get::<String, _>("name"),
        });
        sqlx::query(
            "INSERT INTO webhook_jobs
             (id, webhook_id, notification_id, payload_json, status, attempts, next_attempt_at, created_at, updated_at)
             VALUES (?, ?, ?, ?, 'pending', 0, ?, ?, ?)",
        )
        .bind(id())
        .bind(webhook.get::<String, _>("id"))
        .bind(notification_id)
        .bind(serde_json::to_string(&payload)?)
        .bind(&now)
        .bind(&now)
        .bind(&now)
        .execute(&mut **tx)
        .await?;
    }
    Ok(())
}

struct MentionInput<'a> {
    source_kind: &'a str,
    source_id: &'a str,
    channel_id: Option<&'a str>,
    thread_id: Option<&'a str>,
    conversation_id: Option<&'a str>,
    obj_index: Option<i64>,
    title: &'a str,
    body: &'a str,
}

async fn create_mention_notifications_tx(
    tx: &mut Transaction<'_, Sqlite>,
    actor_id: &str,
    input: MentionInput<'_>,
) -> anyhow::Result<HashSet<String>> {
    let usernames = parse_mentions(input.body);
    let mut targets = HashSet::new();
    for username in usernames {
        let row = sqlx::query(
            "SELECT id FROM accounts
             WHERE lower(username) = lower(?)
               AND activated_at IS NOT NULL
               AND disabled_at IS NULL",
        )
        .bind(&username)
        .fetch_optional(&mut **tx)
        .await?;
        let Some(row) = row else {
            continue;
        };
        let target_id: String = row.get("id");
        if target_id == actor_id || targets.contains(&target_id) {
            continue;
        }
        sqlx::query(
            "INSERT INTO mentions
             (id, target_account_id, actor_account_id, source_kind, source_id, channel_id,
              thread_id, conversation_id, obj_index, created_at)
             VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
        )
        .bind(id())
        .bind(&target_id)
        .bind(actor_id)
        .bind(input.source_kind)
        .bind(input.source_id)
        .bind(input.channel_id)
        .bind(input.thread_id)
        .bind(input.conversation_id)
        .bind(input.obj_index)
        .bind(now())
        .execute(&mut **tx)
        .await?;
        create_notification_tx(
            tx,
            &target_id,
            Some(actor_id),
            NotificationInput {
                kind: "mention",
                source_kind: input.source_kind,
                source_id: input.source_id,
                channel_id: input.channel_id,
                thread_id: input.thread_id,
                conversation_id: input.conversation_id,
                title: input.title,
                body: input.body,
            },
        )
        .await?;
        targets.insert(target_id);
    }
    Ok(targets)
}

struct ReplyNotificationInput<'a> {
    thread_id: &'a str,
    channel_id: &'a str,
    comment_id: &'a str,
    obj_index: i64,
    title: &'a str,
    body: &'a str,
}

async fn create_thread_reply_notifications_tx(
    tx: &mut Transaction<'_, Sqlite>,
    actor_id: &str,
    input: ReplyNotificationInput<'_>,
) -> anyhow::Result<()> {
    let participants = sqlx::query_scalar::<_, String>(
        "SELECT creator_account_id FROM threads WHERE id = ?
         UNION
         SELECT author_account_id FROM comments WHERE thread_id = ? AND deleted_at IS NULL",
    )
    .bind(input.thread_id)
    .bind(input.thread_id)
    .fetch_all(&mut **tx)
    .await?;
    for account_id in participants {
        if account_id == actor_id {
            continue;
        }
        let notification_body = format!("#{}: {}", input.obj_index, input.body);
        create_notification_tx(
            tx,
            &account_id,
            Some(actor_id),
            NotificationInput {
                kind: "reply",
                source_kind: "comment",
                source_id: input.comment_id,
                channel_id: Some(input.channel_id),
                thread_id: Some(input.thread_id),
                conversation_id: None,
                title: input.title,
                body: &notification_body,
            },
        )
        .await?;
    }
    Ok(())
}

async fn create_dm_notifications_tx(
    tx: &mut Transaction<'_, Sqlite>,
    actor_id: &str,
    conversation_id: &str,
    message_id: &str,
    obj_index: i64,
    body: &str,
) -> anyhow::Result<()> {
    let members = sqlx::query_scalar::<_, String>(
        "SELECT account_id FROM conversation_members WHERE conversation_id = ?",
    )
    .bind(conversation_id)
    .fetch_all(&mut **tx)
    .await?;
    for account_id in members {
        if account_id == actor_id {
            continue;
        }
        let notification_body = format!("#{obj_index}: {body}");
        create_notification_tx(
            tx,
            &account_id,
            Some(actor_id),
            NotificationInput {
                kind: "dm",
                source_kind: "dm",
                source_id: message_id,
                channel_id: None,
                thread_id: None,
                conversation_id: Some(conversation_id),
                title: "New DM",
                body: &notification_body,
            },
        )
        .await?;
    }
    Ok(())
}

pub fn parse_mentions(body: &str) -> Vec<String> {
    let mut mentions = Vec::new();
    let mut chars = body.char_indices().peekable();
    while let Some((idx, ch)) = chars.next() {
        if ch != '@' {
            continue;
        }
        if idx > 0
            && body[..idx]
                .chars()
                .next_back()
                .is_some_and(|prev| prev.is_ascii_alphanumeric() || matches!(prev, '_' | '-' | '.'))
        {
            continue;
        }
        let mut raw = String::new();
        while let Some((_, next)) = chars.peek().copied() {
            if next.is_ascii_alphanumeric() || matches!(next, '_' | '-' | '.') {
                raw.push(next);
                chars.next();
            } else {
                break;
            }
        }
        if let Ok(username) = normalize_username(&raw)
            && !mentions.iter().any(|existing| existing == &username)
        {
            mentions.push(username);
        }
    }
    mentions
}

async fn search_visible(
    pool: &SqlitePool,
    account_id: &str,
    query: &str,
    limit: i64,
) -> anyhow::Result<SearchPage> {
    let query = query.trim();
    anyhow::ensure!(!query.is_empty(), "Search query is required");
    let limit = limit.clamp(1, 500);
    let fetch_limit = limit.saturating_add(1);
    let rows = sqlx::query(
        "SELECT search_index.kind, search_index.object_id, search_index.channel_id,
                search_index.thread_id, search_index.conversation_id,
                search_index.title, search_index.body, search_index.context
         FROM search_index
         LEFT JOIN channels c ON c.id = search_index.channel_id
         LEFT JOIN threads t ON t.id = search_index.thread_id
         LEFT JOIN comments cm ON cm.id = search_index.object_id AND search_index.kind = 'comment'
         LEFT JOIN conversation_messages dm ON dm.id = search_index.object_id AND search_index.kind = 'dm'
         WHERE search_index MATCH ?
           AND (
             (search_index.kind IN ('thread', 'comment')
               AND t.deleted_at IS NULL
               AND (cm.id IS NULL OR cm.deleted_at IS NULL)
               AND EXISTS (
                 SELECT 1 FROM channel_members m
                 WHERE m.channel_id = search_index.channel_id AND m.account_id = ?
               ))
             OR
             (search_index.kind = 'dm'
               AND dm.deleted_at IS NULL
               AND EXISTS (
                 SELECT 1 FROM conversation_members m
                 WHERE m.conversation_id = search_index.conversation_id AND m.account_id = ?
               ))
           )
         ORDER BY rank
         LIMIT ?",
    )
    .bind(fts_query(query))
    .bind(account_id)
    .bind(account_id)
    .bind(fetch_limit)
    .fetch_all(pool)
    .await?;
    let mut results = Vec::new();
    for row in rows {
        let kind = match row.get::<String, _>("kind").as_str() {
            "thread" => SearchKind::Thread,
            "comment" => SearchKind::Comment,
            _ => SearchKind::Dm,
        };
        let title: String = row.get("title");
        let body: String = row.get("body");
        let label = match kind {
            SearchKind::Thread => title.clone(),
            SearchKind::Comment => format!("{title} comment"),
            SearchKind::Dm => "DM".to_string(),
        };
        results.push(SearchResult {
            kind,
            label,
            context: row.get("context"),
            snippet: snippet(&format!("{title}\n{body}"), query),
            channel_id: row.get("channel_id"),
            thread_id: row.get("thread_id"),
            conversation_id: row.get("conversation_id"),
        });
    }
    let has_more = results.len() > limit as usize;
    results.truncate(limit as usize);
    Ok(SearchPage { results, has_more })
}

fn publish(live_tx: &broadcast::Sender<LiveEvent>, event: LiveEvent) {
    let _ = live_tx.send(event);
}

fn account_from_row(row: sqlx::sqlite::SqliteRow) -> Account {
    let activated: Option<String> = row.get("activated_at");
    Account {
        id: row.get("id"),
        username: row.get("username"),
        display_name: row.get("display_name"),
        role: Role::from_db(row.get::<String, _>("role").as_str()),
        activated: activated.is_some(),
    }
}

fn ssh_key_summary_from_row(row: sqlx::sqlite::SqliteRow) -> SshKeySummary {
    SshKeySummary {
        id: row.get("id"),
        username: row.get("username"),
        fingerprint: row.get("fingerprint"),
        label: row.get("label"),
        created_at: row.get("created_at"),
        last_used_at: row.get("last_used_at"),
        revoked_at: row.get("revoked_at"),
    }
}

struct ParsedPublicKey {
    fingerprint: String,
    public_key: String,
}

fn parse_public_key(public_key: &str) -> anyhow::Result<ParsedPublicKey> {
    let key = russh::keys::PublicKey::from_openssh(public_key.trim())
        .context("public key must be an OpenSSH public key")?;
    Ok(ParsedPublicKey {
        fingerprint: key.fingerprint(russh::keys::HashAlg::Sha256).to_string(),
        public_key: key.to_openssh().context("serializing public key")?,
    })
}

fn rows_to_json(rows: Vec<sqlx::sqlite::SqliteRow>) -> anyhow::Result<serde_json::Value> {
    let mut out = Vec::new();
    for row in rows {
        let mut object = serde_json::Map::new();
        for column in row.columns() {
            let name = column.name();
            let value = if let Ok(value) = row.try_get::<Option<String>, _>(name) {
                value
                    .map(serde_json::Value::String)
                    .unwrap_or(serde_json::Value::Null)
            } else if let Ok(value) = row.try_get::<Option<i64>, _>(name) {
                value
                    .map(|value| serde_json::Value::Number(value.into()))
                    .unwrap_or(serde_json::Value::Null)
            } else {
                serde_json::Value::Null
            };
            object.insert(name.to_string(), value);
        }
        out.push(serde_json::Value::Object(object));
    }
    Ok(serde_json::Value::Array(out))
}

fn export_markdown(bundle: &serde_json::Value) -> String {
    let mut out = String::from("# sshoosh export\n\n");
    if let Some(exported_at) = bundle
        .get("exported_at")
        .and_then(serde_json::Value::as_str)
    {
        out.push_str(&format!("Exported at `{exported_at}`.\n\n"));
    }
    for section in [
        "users",
        "channels",
        "threads",
        "comments",
        "dms",
        "dm_messages",
        "mentions",
        "reactions",
        "notifications",
        "webhooks",
        "audit",
    ] {
        let rows = bundle
            .get(section)
            .and_then(serde_json::Value::as_array)
            .cloned()
            .unwrap_or_default();
        out.push_str(&format!("## {section}\n\n"));
        if rows.is_empty() {
            out.push_str("_No rows._\n\n");
            continue;
        }
        for row in rows {
            out.push_str("- ");
            out.push_str(&compact_json(&row));
            out.push('\n');
        }
        out.push('\n');
    }
    out
}

fn compact_json(value: &serde_json::Value) -> String {
    serde_json::to_string(value).unwrap_or_else(|_| "{}".to_string())
}

async fn next_username(tx: &mut Transaction<'_, Sqlite>, desired: &str) -> anyhow::Result<String> {
    let base = normalize_username(desired).unwrap_or_else(|_| "user".to_string());
    let mut candidate = base.clone();
    let mut suffix = 2;
    loop {
        let exists: i64 =
            sqlx::query_scalar("SELECT COUNT(*) FROM accounts WHERE lower(username) = lower(?)")
                .bind(&candidate)
                .fetch_one(&mut **tx)
                .await?;
        if exists == 0 {
            return Ok(candidate);
        }
        candidate = format!("{base}-{suffix}");
        suffix += 1;
    }
}

fn normalize_username(input: &str) -> anyhow::Result<String> {
    let mut out = String::new();
    for ch in input.trim().to_lowercase().chars() {
        if ch.is_ascii_alphanumeric() {
            out.push(ch);
        } else if matches!(ch, '-' | '_' | '.') && !out.ends_with('-') {
            out.push('-');
        }
    }
    let out = out.trim_matches('-').to_string();
    anyhow::ensure!(
        (2..=32).contains(&out.len()),
        "Username must be 2-32 characters"
    );
    Ok(out)
}

fn normalize_slug(input: &str) -> anyhow::Result<String> {
    let out = normalize_name_key(input);
    anyhow::ensure!(
        (2..=48).contains(&out.len()),
        "Channel name must be 2-48 characters"
    );
    Ok(out)
}

fn normalize_name_key(input: &str) -> String {
    let mut out = String::new();
    for ch in input.trim().to_lowercase().chars() {
        if ch.is_ascii_alphanumeric() {
            out.push(ch);
        } else if matches!(ch, '-' | '_' | '.' | ' ') && !out.ends_with('-') {
            out.push('-');
        }
    }
    out.trim_matches('-').to_string()
}

fn id() -> String {
    Uuid::now_v7().to_string()
}

fn now() -> String {
    time::OffsetDateTime::now_utc()
        .format(&time::format_description::well_known::Rfc3339)
        .expect("format timestamp")
}

fn timestamp_after_hours(hours: i64) -> Option<String> {
    if hours <= 0 {
        return None;
    }
    (time::OffsetDateTime::now_utc() + time::Duration::hours(hours))
        .format(&time::format_description::well_known::Rfc3339)
        .ok()
}

fn snippet(text: &str, query: &str) -> String {
    let text = text.replace('\n', " ");
    let lower = text.to_lowercase();
    let needle = query.to_lowercase();
    let start = lower
        .find(&needle)
        .map(|idx| idx.saturating_sub(32))
        .unwrap_or(0);
    let mut out = text.chars().skip(start).take(140).collect::<String>();
    if start > 0 {
        out.insert_str(0, "...");
    }
    if text.chars().count() > start + out.chars().count() {
        out.push_str("...");
    }
    out
}

fn invite_code() -> String {
    let mut bytes = [0u8; 18];
    rand::thread_rng().fill_bytes(&mut bytes);
    URL_SAFE_NO_PAD.encode(bytes)
}

fn code_hash(code: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(code.trim().as_bytes());
    format!("{:x}", hasher.finalize())
}

fn dm_key(a: &str, b: &str) -> String {
    let mut ids = [a.to_string(), b.to_string()];
    ids.sort();
    format!("{}:{}", ids[0], ids[1])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn recent_but_disconnected_presence_is_not_online() {
        let recent = now();
        let presence = UserPresence {
            username: "alice".to_string(),
            display_name: "Alice".to_string(),
            last_seen_at: Some(recent.clone()),
            connected: false,
        };
        assert_eq!(presence.state(), PresenceState::Away);

        let presence = UserPresence {
            connected: true,
            last_seen_at: Some(recent),
            ..presence
        };
        assert_eq!(presence.state(), PresenceState::Online);
    }
}
