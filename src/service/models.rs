pub const DEFAULT_HISTORY_LIMIT: i64 = 500;
pub const MAX_HISTORY_LIMIT: i64 = 5_000;
const PRESENCE_SESSION_TTL_SECONDS: i64 = 120;

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

