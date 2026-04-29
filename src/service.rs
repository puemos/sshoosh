use anyhow::{Context, bail};
use base64::{Engine as _, engine::general_purpose::URL_SAFE_NO_PAD};
use rand::RngCore;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use sqlx::{Row, Sqlite, SqlitePool, Transaction};
use tokio::sync::{broadcast, mpsc, oneshot};
use uuid::Uuid;

use crate::db::Database;

#[derive(Clone)]
pub struct ServerState {
    pub db: Database,
    writer: WriteHandle,
    live_tx: broadcast::Sender<LiveEvent>,
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

    fn from_str(value: &str) -> Self {
        match value {
            "owner" => Self::Owner,
            "admin" => Self::Admin,
            _ => Self::Member,
        }
    }

    fn can_admin(self) -> bool {
        matches!(self, Role::Owner | Role::Admin)
    }
}

#[derive(Clone, Debug)]
pub struct Channel {
    pub id: String,
    pub slug: String,
    pub name: String,
    pub visibility: String,
    pub topic: Option<String>,
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
}

#[derive(Clone, Debug)]
pub struct CommentItem {
    pub id: String,
    pub author: String,
    pub obj_index: i64,
    pub body: String,
}

#[derive(Clone, Debug)]
pub struct Conversation {
    pub id: String,
    pub peer_username: String,
    pub last_message_index: i64,
    pub unread_count: i64,
}

#[derive(Clone, Debug)]
pub struct ConversationMessage {
    pub author: String,
    pub obj_index: i64,
    pub body: String,
}

#[derive(Clone, Debug, Default)]
pub struct Snapshot {
    pub channels: Vec<Channel>,
    pub threads: Vec<ThreadItem>,
    pub comments: Vec<CommentItem>,
    pub conversations: Vec<Conversation>,
    pub conversation_messages: Vec<ConversationMessage>,
    pub selected_channel_id: Option<String>,
    pub selected_thread_id: Option<String>,
    pub selected_conversation_id: Option<String>,
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
        let state = Self {
            db: db.clone(),
            writer: WriteHandle { tx },
            live_tx,
        };
        start_writer(db.write_pool().clone(), state.live_tx.clone(), rx);
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
        let account = self.reload_account(account_id).await?;
        if !account.activated {
            return Ok(Snapshot::default());
        }

        let channels = load_channels(self.db.read_pool(), account_id).await?;
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
        let comments = if let Some(thread_id) = selected_thread_id.as_deref() {
            load_comments(self.db.read_pool(), thread_id).await?
        } else {
            Vec::new()
        };

        let conversations = load_conversations(self.db.read_pool(), account_id).await?;
        let selected_conversation_id = selected_conversation_id
            .filter(|id| {
                conversations
                    .iter()
                    .any(|conversation| conversation.id == *id)
            })
            .map(ToOwned::to_owned);
        let conversation_messages =
            if let Some(conversation_id) = selected_conversation_id.as_deref() {
                load_conversation_messages(self.db.read_pool(), conversation_id).await?
            } else {
                Vec::new()
            };

        Ok(Snapshot {
            channels,
            threads,
            comments,
            conversations,
            conversation_messages,
            selected_channel_id,
            selected_thread_id,
            selected_conversation_id,
        })
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

async fn create_invite(
    pool: &SqlitePool,
    live_tx: &broadcast::Sender<LiveEvent>,
    actor_id: &str,
) -> anyhow::Result<String> {
    let mut tx = begin(pool).await?;
    let actor = load_account_tx(&mut tx, actor_id).await?;
    anyhow::ensure!(
        actor.role.can_admin(),
        "Only owners/admins can create invites"
    );
    let code = invite_code();
    let code_hash = code_hash(&code);
    let now = now();
    sqlx::query(
        "INSERT INTO invites
         (id, code_hash, role_on_accept, created_by_account_id, created_at)
         VALUES (?, ?, 'member', ?, ?)",
    )
    .bind(id())
    .bind(code_hash)
    .bind(actor_id)
    .bind(&now)
    .execute(&mut *tx)
    .await?;
    let event = insert_event(
        &mut tx,
        None,
        None,
        None,
        "invite.created",
        serde_json::json!({"actor_id": actor_id}),
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
    if private {
        sqlx::query(
            "INSERT INTO channel_members (channel_id, account_id, role, joined_at)
             VALUES (?, ?, 'member', ?)",
        )
        .bind(&channel_id)
        .bind(actor_id)
        .bind(&now)
        .execute(&mut *tx)
        .await?;
    }
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
    body: &str,
) -> anyhow::Result<String> {
    anyhow::ensure!(!title.trim().is_empty(), "Thread title is required");
    anyhow::ensure!(!body.trim().is_empty(), "Thread body is required");
    let mut tx = begin(pool).await?;
    ensure_can_view_channel(&mut tx, actor_id, channel_id).await?;
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
    .bind(title.trim())
    .bind(body.trim())
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
    let event = insert_event(
        &mut tx,
        Some(channel_id),
        Some(&thread_id),
        None,
        "thread.created",
        serde_json::json!({"thread_id": thread_id, "channel_id": channel_id, "title": title.trim()}),
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
    sqlx::query(
        "INSERT INTO comments
         (id, thread_id, channel_id, author_account_id, obj_index, body, created_at, updated_at)
         VALUES (?, ?, ?, ?, ?, ?, ?, ?)",
    )
    .bind(id())
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
    sqlx::query(
        "INSERT INTO conversation_messages
         (id, conversation_id, author_account_id, obj_index, body, created_at, updated_at)
         VALUES (?, ?, ?, ?, ?, ?, ?)",
    )
    .bind(id())
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

async fn begin(pool: &SqlitePool) -> anyhow::Result<Transaction<'_, Sqlite>> {
    let tx = pool.begin().await?;
    Ok(tx)
}

async fn load_channels(pool: &SqlitePool, account_id: &str) -> anyhow::Result<Vec<Channel>> {
    let rows = sqlx::query(
        "SELECT c.id, c.slug, c.name, c.visibility, c.topic
         FROM channels c
         WHERE c.archived_at IS NULL
           AND (
             c.visibility = 'public'
             OR EXISTS (
                SELECT 1 FROM channel_members m
                WHERE m.channel_id = c.id AND m.account_id = ?
             )
           )
         ORDER BY CASE WHEN c.slug = 'general' THEN 0 ELSE 1 END, c.slug",
    )
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
                MAX(t.last_comment_index - COALESCE(r.last_read_index, 0), 0) AS unread_count
         FROM threads t
         JOIN accounts a ON a.id = t.creator_account_id
         LEFT JOIN thread_reads r ON r.thread_id = t.id AND r.account_id = ?
         WHERE t.channel_id = ? AND t.deleted_at IS NULL
         ORDER BY t.last_activity_at DESC, t.id DESC
         LIMIT 200",
    )
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
        })
        .collect())
}

async fn load_comments(pool: &SqlitePool, thread_id: &str) -> anyhow::Result<Vec<CommentItem>> {
    let rows = sqlx::query(
        "SELECT c.id, a.username AS author, c.obj_index, c.body
         FROM comments c
         JOIN accounts a ON a.id = c.author_account_id
         WHERE c.thread_id = ? AND c.deleted_at IS NULL
         ORDER BY c.obj_index ASC
         LIMIT 500",
    )
    .bind(thread_id)
    .fetch_all(pool)
    .await?;
    Ok(rows
        .into_iter()
        .map(|row| CommentItem {
            id: row.get("id"),
            author: row.get("author"),
            obj_index: row.get("obj_index"),
            body: row.get("body"),
        })
        .collect())
}

async fn load_conversations(
    pool: &SqlitePool,
    account_id: &str,
) -> anyhow::Result<Vec<Conversation>> {
    let rows = sqlx::query(
        "SELECT c.id,
                peer.username AS peer_username,
                c.last_message_index,
                MAX(c.last_message_index - me.last_read_index, 0) AS unread_count
         FROM conversations c
         JOIN conversation_members me ON me.conversation_id = c.id AND me.account_id = ?
         JOIN conversation_members other ON other.conversation_id = c.id AND other.account_id <> ?
         JOIN accounts peer ON peer.id = other.account_id
         WHERE c.archived_at IS NULL
         ORDER BY c.last_activity_at DESC",
    )
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
        })
        .collect())
}

async fn load_conversation_messages(
    pool: &SqlitePool,
    conversation_id: &str,
) -> anyhow::Result<Vec<ConversationMessage>> {
    let rows = sqlx::query(
        "SELECT a.username AS author, m.obj_index, m.body
         FROM conversation_messages m
         JOIN accounts a ON a.id = m.author_account_id
         WHERE m.conversation_id = ? AND m.deleted_at IS NULL
         ORDER BY m.obj_index ASC
         LIMIT 500",
    )
    .bind(conversation_id)
    .fetch_all(pool)
    .await?;
    Ok(rows
        .into_iter()
        .map(|row| ConversationMessage {
            author: row.get("author"),
            obj_index: row.get("obj_index"),
            body: row.get("body"),
        })
        .collect())
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
           AND (
             c.visibility = 'public'
             OR EXISTS (
                SELECT 1 FROM channel_members m
                WHERE m.channel_id = c.id AND m.account_id = ?
             )
           )",
    )
    .bind(channel_id)
    .bind(account_id)
    .fetch_one(&mut **tx)
    .await?;
    anyhow::ensure!(count > 0, "You do not have access to this channel");
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

fn publish(live_tx: &broadcast::Sender<LiveEvent>, event: LiveEvent) {
    let _ = live_tx.send(event);
}

fn account_from_row(row: sqlx::sqlite::SqliteRow) -> Account {
    let activated: Option<String> = row.get("activated_at");
    Account {
        id: row.get("id"),
        username: row.get("username"),
        display_name: row.get("display_name"),
        role: Role::from_str(row.get::<String, _>("role").as_str()),
        activated: activated.is_some(),
    }
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
    let mut out = String::new();
    for ch in input.trim().to_lowercase().chars() {
        if ch.is_ascii_alphanumeric() {
            out.push(ch);
        } else if matches!(ch, '-' | '_' | '.' | ' ') && !out.ends_with('-') {
            out.push('-');
        }
    }
    let out = out.trim_matches('-').to_string();
    anyhow::ensure!(
        (2..=48).contains(&out.len()),
        "Channel name must be 2-48 characters"
    );
    Ok(out)
}

fn id() -> String {
    Uuid::now_v7().to_string()
}

fn now() -> String {
    time::OffsetDateTime::now_utc()
        .format(&time::format_description::well_known::Rfc3339)
        .expect("format timestamp")
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
