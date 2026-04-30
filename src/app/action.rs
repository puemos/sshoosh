use crate::service::Role;

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Action {
    CreateInvite,
    CreateInviteWithOptions {
        role: Role,
        ttl_hours: Option<i64>,
    },
    AcceptInvite {
        code: String,
        username: String,
    },
    CreateChannel {
        name: String,
        private: bool,
    },
    JoinChannel {
        slug: String,
    },
    LeaveChannel {
        slug: Option<String>,
    },
    ListChannels,
    RenameChannel {
        slug: Option<String>,
        name: String,
    },
    SetChannelTopic {
        slug: Option<String>,
        topic: String,
    },
    SetChannelArchived {
        slug: Option<String>,
        archived: bool,
    },
    CreateThread {
        title: String,
        body: String,
    },
    AddComment {
        body: String,
    },
    OpenDm {
        target: String,
    },
    SendDm {
        body: String,
    },
    MarkThreadRead,
    MarkThreadUnread,
    MarkDmRead,
    MarkDmUnread,
    NextUnread,
    ListUsers,
    SetUsername {
        username: String,
    },
    SetProfile {
        display_name: String,
    },
    SetUserDisabled {
        username: String,
        disabled: bool,
    },
    SetUserRole {
        username: String,
        role: Role,
    },
    ListKeys,
    ListMyKeys,
    AddKey {
        public_key: String,
        label: Option<String>,
    },
    LabelKey {
        key: String,
        label: String,
    },
    RevokeKey {
        key: String,
    },
    ListInvites,
    RevokeInvite {
        invite_id: String,
    },
    ListChannelMembers {
        slug: String,
    },
    AddChannelMember {
        slug: String,
        username: String,
    },
    RemoveChannelMember {
        slug: String,
        username: String,
    },
    RenameThread {
        title: String,
    },
    DeleteThread,
    SetThreadArchived {
        archived: bool,
    },
    SetThreadPinned {
        pinned: bool,
    },
    SetThreadMuted {
        ttl_hours: Option<i64>,
    },
    SetThreadSaved {
        saved: bool,
    },
    EditComment {
        index: i64,
        body: String,
    },
    DeleteComment {
        index: i64,
    },
    EditDm {
        index: i64,
        body: String,
    },
    DeleteDm {
        index: i64,
    },
    SetDmMuted {
        ttl_hours: Option<i64>,
    },
    SetDmSaved {
        saved: bool,
    },
    React {
        emoji: String,
        index: Option<i64>,
    },
    Unreact {
        emoji: String,
        index: Option<i64>,
    },
    ListMentions,
    ListNotifications,
    MarkNotificationRead {
        notification_id: Option<String>,
    },
    ListWebhooks,
    AddWebhook {
        name: String,
        url: String,
    },
    RemoveWebhook {
        webhook_id: String,
    },
    ListAudit,
    Search {
        query: String,
    },
    LoadMore,
    LoadOlder,
}
