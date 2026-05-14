use crate::features::accounts::model::Role;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SourceTarget {
    pub channel_id: Option<String>,
    pub channel_slug: Option<String>,
    pub thread_id: Option<String>,
    pub conversation_id: Option<String>,
    pub focus: Option<SourceFocus>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SourceFocus {
    ThreadRoot,
    Comment(i64),
    Dm(i64),
}

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum LoadMoreRequest {
    Search { query: String, cursor: String },
    Label { tag: String, cursor: String },
    Saved { cursor: String },
    Notifications { cursor: String },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum ActionDomain {
    App,
    Accounts,
    Channels,
    Messages,
    Notifications,
    Audit,
    Feeds,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Action {
    CreateInvite,
    CreateInviteWithOptions {
        role: Role,
        ttl_hours: Option<i64>,
    },
    CompleteOnboarding {
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
    OpenAccount,
    SaveAccountSettings {
        username: String,
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
    CreateDeviceLinkToken {
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
    SetMessageSaved {
        index: i64,
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
    OpenSourceTarget {
        target: SourceTarget,
    },
    MarkNotificationRead {
        notification_id: Option<String>,
    },
    ArchiveNotifications,
    SetTerminalNotifications {
        enabled: bool,
    },
    ShowTerminalNotificationsStatus,
    ListAudit,
    Search {
        query: String,
    },
    OpenLabel {
        tag: String,
    },
    ListSaved,
    LoadMore {
        request: Option<LoadMoreRequest>,
    },
    LoadOlder,
}

impl Action {
    pub(crate) fn domain(&self) -> ActionDomain {
        match self {
            Self::OpenAccount => ActionDomain::App,
            Self::CreateInvite
            | Self::CreateInviteWithOptions { .. }
            | Self::CompleteOnboarding { .. }
            | Self::ListUsers
            | Self::SetUsername { .. }
            | Self::SetProfile { .. }
            | Self::SaveAccountSettings { .. }
            | Self::SetUserDisabled { .. }
            | Self::SetUserRole { .. }
            | Self::ListKeys
            | Self::ListMyKeys
            | Self::AddKey { .. }
            | Self::CreateDeviceLinkToken { .. }
            | Self::LabelKey { .. }
            | Self::RevokeKey { .. }
            | Self::ListInvites
            | Self::RevokeInvite { .. } => ActionDomain::Accounts,
            Self::CreateChannel { .. }
            | Self::JoinChannel { .. }
            | Self::LeaveChannel { .. }
            | Self::ListChannels
            | Self::RenameChannel { .. }
            | Self::SetChannelTopic { .. }
            | Self::SetChannelArchived { .. }
            | Self::ListChannelMembers { .. }
            | Self::AddChannelMember { .. }
            | Self::RemoveChannelMember { .. } => ActionDomain::Channels,
            Self::CreateThread { .. }
            | Self::AddComment { .. }
            | Self::OpenDm { .. }
            | Self::SendDm { .. }
            | Self::MarkThreadRead
            | Self::MarkThreadUnread
            | Self::MarkDmRead
            | Self::MarkDmUnread
            | Self::NextUnread
            | Self::RenameThread { .. }
            | Self::DeleteThread
            | Self::SetThreadArchived { .. }
            | Self::SetThreadPinned { .. }
            | Self::SetThreadMuted { .. }
            | Self::EditComment { .. }
            | Self::DeleteComment { .. }
            | Self::EditDm { .. }
            | Self::DeleteDm { .. }
            | Self::SetDmMuted { .. }
            | Self::SetMessageSaved { .. }
            | Self::React { .. }
            | Self::Unreact { .. } => ActionDomain::Messages,
            Self::ListMentions
            | Self::ListNotifications
            | Self::OpenSourceTarget { .. }
            | Self::MarkNotificationRead { .. }
            | Self::ArchiveNotifications
            | Self::SetTerminalNotifications { .. }
            | Self::ShowTerminalNotificationsStatus => ActionDomain::Notifications,
            Self::ListAudit => ActionDomain::Audit,
            Self::Search { .. }
            | Self::OpenLabel { .. }
            | Self::ListSaved
            | Self::LoadMore { .. }
            | Self::LoadOlder => ActionDomain::Feeds,
        }
    }

    pub(crate) fn refreshes_after(&self) -> bool {
        !matches!(
            self,
            Self::LoadMore { .. }
                | Self::Search { .. }
                | Self::OpenLabel { .. }
                | Self::OpenAccount
                | Self::ListSaved
                | Self::ListNotifications
        )
    }
}
