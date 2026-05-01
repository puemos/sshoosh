use super::{Channel, NotificationSummary, PresenceState, SearchResult, UserPresence};

#[derive(Clone, Debug)]
pub struct ReactionSummary {
    pub emoji: String,
    pub count: i64,
    pub reacted_by_me: bool,
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
    pub reactions: Vec<ReactionSummary>,
}

#[derive(Clone, Debug)]
pub struct CommentItem {
    pub id: String,
    pub author: String,
    pub obj_index: i64,
    pub body: String,
    pub created_at: String,
    pub edited_at: Option<String>,
    pub saved_at: Option<String>,
    pub reactions: Vec<ReactionSummary>,
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
pub struct DmSidebarItem {
    pub conversation_id: Option<String>,
    pub peer_username: String,
    pub last_message_index: i64,
    pub unread_count: i64,
    pub last_activity_at: Option<String>,
    pub last_message_preview: Option<String>,
    pub muted_until: Option<String>,
    pub saved_at: Option<String>,
}

impl From<&Conversation> for DmSidebarItem {
    fn from(conversation: &Conversation) -> Self {
        Self {
            conversation_id: Some(conversation.id.clone()),
            peer_username: conversation.peer_username.clone(),
            last_message_index: conversation.last_message_index,
            unread_count: conversation.unread_count,
            last_activity_at: conversation.last_activity_at.clone(),
            last_message_preview: conversation.last_message_preview.clone(),
            muted_until: conversation.muted_until.clone(),
            saved_at: conversation.saved_at.clone(),
        }
    }
}

#[derive(Clone, Debug)]
pub struct ConversationMessage {
    pub id: String,
    pub author: String,
    pub obj_index: i64,
    pub body: String,
    pub created_at: String,
    pub edited_at: Option<String>,
    pub saved_at: Option<String>,
    pub reactions: Vec<ReactionSummary>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SavedMessageKind {
    Comment,
    Dm,
}

#[derive(Clone, Debug)]
pub struct SavedMessageItem {
    pub kind: SavedMessageKind,
    pub source_id: String,
    pub source_obj_index: i64,
    pub author: String,
    pub body: String,
    pub source_label: String,
    pub channel_slug: Option<String>,
    pub thread_title: Option<String>,
    pub dm_peer_username: Option<String>,
    pub saved_at: String,
    pub created_at: String,
    pub channel_id: Option<String>,
    pub thread_id: Option<String>,
    pub conversation_id: Option<String>,
}

#[derive(Clone, Debug, Default)]
pub struct Snapshot {
    pub current_username: Option<String>,
    pub users: Vec<UserPresence>,
    pub channels: Vec<Channel>,
    pub threads: Vec<ThreadItem>,
    pub comments: Vec<CommentItem>,
    pub conversations: Vec<Conversation>,
    pub dm_sidebar: Vec<DmSidebarItem>,
    pub conversation_messages: Vec<ConversationMessage>,
    pub comments_has_more: bool,
    pub conversation_messages_has_more: bool,
    pub search_query: Option<String>,
    pub search_results: Vec<SearchResult>,
    pub search_next_cursor: Option<String>,
    pub search_has_more: bool,
    pub saved_messages: Vec<SavedMessageItem>,
    pub saved_next_cursor: Option<String>,
    pub saved_count: i64,
    pub saved_has_more: bool,
    pub notifications: Vec<NotificationSummary>,
    pub notifications_next_cursor: Option<String>,
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
