use crate::{domain::*, service::ServerState};

#[derive(Clone)]
pub struct ClientSession {
    account: Account,
    state: ServerState,
}

impl ClientSession {
    pub fn new(account: Account, state: ServerState) -> Self {
        Self { account, state }
    }

    pub fn account(&self) -> &Account {
        &self.account
    }

    pub fn subscribe(&self) -> tokio::sync::broadcast::Receiver<LiveEvent> {
        self.state.subscribe()
    }

    pub async fn refresh_account(&mut self) -> anyhow::Result<Account> {
        self.account = self.state.reload_account(&self.account.id).await?;
        Ok(self.account.clone())
    }

    pub async fn snapshot(
        &self,
        selected_channel_id: Option<&str>,
        selected_thread_id: Option<&str>,
        selected_conversation_id: Option<&str>,
        history_limit: i64,
    ) -> anyhow::Result<Snapshot> {
        self.state
            .snapshot_with_history_limit(
                &self.account.id,
                selected_channel_id,
                selected_thread_id,
                selected_conversation_id,
                history_limit,
            )
            .await
    }

    pub async fn create_invite(&self, actor_id: String) -> anyhow::Result<String> {
        self.state.create_invite(actor_id).await
    }

    pub async fn create_invite_with_options(
        &self,
        actor_id: &str,
        role: Role,
        ttl_hours: Option<i64>,
    ) -> anyhow::Result<String> {
        self.state
            .create_invite_with_options(actor_id, role, ttl_hours)
            .await
    }

    pub async fn accept_invite(
        &self,
        account_id: String,
        code: String,
        username: String,
    ) -> anyhow::Result<()> {
        self.state.accept_invite(account_id, code, username).await
    }

    pub async fn create_channel(
        &self,
        actor_id: String,
        name: String,
        private: bool,
    ) -> anyhow::Result<String> {
        self.state.create_channel(actor_id, name, private).await
    }

    pub async fn join_channel(&self, actor_id: String, slug: String) -> anyhow::Result<String> {
        self.state.join_channel(actor_id, slug).await
    }

    pub async fn leave_channel(&self, actor_id: &str, slug: &str) -> anyhow::Result<()> {
        self.state.leave_channel(actor_id, slug).await
    }

    pub async fn list_channels(
        &self,
        actor_id: &str,
        include_archived: bool,
    ) -> anyhow::Result<Vec<ChannelDirectoryItem>> {
        self.state.list_channels(actor_id, include_archived).await
    }

    pub async fn rename_channel(
        &self,
        actor_id: &str,
        slug: &str,
        name: &str,
    ) -> anyhow::Result<()> {
        self.state.rename_channel(actor_id, slug, name).await
    }

    pub async fn set_channel_topic(
        &self,
        actor_id: &str,
        slug: &str,
        topic: &str,
    ) -> anyhow::Result<()> {
        self.state.set_channel_topic(actor_id, slug, topic).await
    }

    pub async fn set_channel_archived(
        &self,
        actor_id: &str,
        slug: &str,
        archived: bool,
    ) -> anyhow::Result<()> {
        self.state
            .set_channel_archived(actor_id, slug, archived)
            .await
    }

    pub async fn create_thread(
        &self,
        actor_id: String,
        channel_id: String,
        title: String,
    ) -> anyhow::Result<String> {
        self.state.create_thread(actor_id, channel_id, title).await
    }

    pub async fn add_comment(
        &self,
        actor_id: String,
        thread_id: String,
        body: String,
    ) -> anyhow::Result<()> {
        self.state.add_comment(actor_id, thread_id, body).await
    }

    pub async fn open_dm(&self, actor_id: String, target: String) -> anyhow::Result<String> {
        self.state.open_dm(actor_id, target).await
    }

    pub async fn send_dm(
        &self,
        actor_id: String,
        conversation_id: String,
        body: String,
    ) -> anyhow::Result<()> {
        self.state.send_dm(actor_id, conversation_id, body).await
    }

    pub async fn mark_thread_read(&self, account_id: &str, thread_id: &str) -> anyhow::Result<()> {
        self.state.mark_thread_read(account_id, thread_id).await
    }

    pub async fn mark_thread_unread(
        &self,
        account_id: &str,
        thread_id: &str,
    ) -> anyhow::Result<()> {
        self.state.mark_thread_unread(account_id, thread_id).await
    }

    pub async fn mark_conversation_read(
        &self,
        account_id: &str,
        conversation_id: &str,
    ) -> anyhow::Result<()> {
        self.state
            .mark_conversation_read(account_id, conversation_id)
            .await
    }

    pub async fn mark_conversation_unread(
        &self,
        account_id: &str,
        conversation_id: &str,
    ) -> anyhow::Result<()> {
        self.state
            .mark_conversation_unread(account_id, conversation_id)
            .await
    }

    pub async fn next_unread(&self, account_id: &str) -> anyhow::Result<Option<NextUnread>> {
        self.state.next_unread(account_id).await
    }

    pub async fn list_accounts(&self, actor_id: &str) -> anyhow::Result<Vec<AccountSummary>> {
        self.state.list_accounts(actor_id).await
    }

    pub async fn rename_user(
        &self,
        actor_id: &str,
        username: &str,
        next: &str,
    ) -> anyhow::Result<()> {
        self.state.rename_user(actor_id, username, next).await
    }

    pub async fn set_display_name(
        &self,
        actor_id: &str,
        username: &str,
        display_name: &str,
    ) -> anyhow::Result<()> {
        self.state
            .set_display_name(actor_id, username, display_name)
            .await
    }

    pub async fn set_user_disabled(
        &self,
        actor_id: &str,
        username: &str,
        disabled: bool,
    ) -> anyhow::Result<()> {
        self.state
            .set_user_disabled(actor_id, username, disabled)
            .await
    }

    pub async fn set_user_role(
        &self,
        actor_id: &str,
        username: &str,
        role: Role,
    ) -> anyhow::Result<()> {
        self.state.set_user_role(actor_id, username, role).await
    }

    pub async fn list_ssh_keys(&self, actor_id: &str) -> anyhow::Result<Vec<SshKeySummary>> {
        self.state.list_ssh_keys(actor_id).await
    }

    pub async fn list_my_ssh_keys(&self, account_id: &str) -> anyhow::Result<Vec<SshKeySummary>> {
        self.state.list_my_ssh_keys(account_id).await
    }

    pub async fn add_ssh_key(
        &self,
        actor_id: &str,
        username: Option<&str>,
        public_key: &str,
        label: Option<&str>,
    ) -> anyhow::Result<SshKeySummary> {
        self.state
            .add_ssh_key(actor_id, username, public_key, label)
            .await
    }

    pub async fn label_ssh_key(
        &self,
        actor_id: &str,
        key: &str,
        label: &str,
    ) -> anyhow::Result<()> {
        self.state.label_ssh_key(actor_id, key, label).await
    }

    pub async fn revoke_ssh_key(&self, actor_id: &str, key: &str) -> anyhow::Result<()> {
        self.state.revoke_ssh_key(actor_id, key).await
    }

    pub async fn list_invites(&self, actor_id: &str) -> anyhow::Result<Vec<InviteSummary>> {
        self.state.list_invites(actor_id).await
    }

    pub async fn revoke_invite(&self, actor_id: &str, invite_id: &str) -> anyhow::Result<()> {
        self.state.revoke_invite(actor_id, invite_id).await
    }

    pub async fn list_channel_members(
        &self,
        actor_id: &str,
        slug: &str,
    ) -> anyhow::Result<Vec<ChannelMemberSummary>> {
        self.state.list_channel_members(actor_id, slug).await
    }

    pub async fn add_channel_member(
        &self,
        actor_id: &str,
        slug: &str,
        username: &str,
    ) -> anyhow::Result<()> {
        self.state
            .add_channel_member(actor_id, slug, username)
            .await
    }

    pub async fn remove_channel_member(
        &self,
        actor_id: &str,
        slug: &str,
        username: &str,
    ) -> anyhow::Result<()> {
        self.state
            .remove_channel_member(actor_id, slug, username)
            .await
    }

    pub async fn rename_thread(
        &self,
        actor_id: &str,
        thread_id: &str,
        title: &str,
    ) -> anyhow::Result<()> {
        self.state.rename_thread(actor_id, thread_id, title).await
    }

    pub async fn delete_thread(&self, actor_id: &str, thread_id: &str) -> anyhow::Result<()> {
        self.state.delete_thread(actor_id, thread_id).await
    }

    pub async fn set_thread_archived(
        &self,
        actor_id: &str,
        thread_id: &str,
        archived: bool,
    ) -> anyhow::Result<()> {
        self.state
            .set_thread_archived(actor_id, thread_id, archived)
            .await
    }

    pub async fn set_thread_pinned(
        &self,
        actor_id: &str,
        thread_id: &str,
        pinned: bool,
    ) -> anyhow::Result<()> {
        self.state
            .set_thread_pinned(actor_id, thread_id, pinned)
            .await
    }

    pub async fn set_thread_muted(
        &self,
        actor_id: &str,
        thread_id: &str,
        ttl_hours: Option<i64>,
    ) -> anyhow::Result<()> {
        self.state
            .set_thread_muted(actor_id, thread_id, ttl_hours)
            .await
    }

    pub async fn set_thread_saved(
        &self,
        actor_id: &str,
        thread_id: &str,
        saved: bool,
    ) -> anyhow::Result<()> {
        self.state
            .set_thread_saved(actor_id, thread_id, saved)
            .await
    }

    pub async fn edit_comment(
        &self,
        actor_id: &str,
        thread_id: &str,
        index: i64,
        body: &str,
    ) -> anyhow::Result<()> {
        self.state
            .edit_comment(actor_id, thread_id, index, body)
            .await
    }

    pub async fn delete_comment(
        &self,
        actor_id: &str,
        thread_id: &str,
        index: i64,
    ) -> anyhow::Result<()> {
        self.state.delete_comment(actor_id, thread_id, index).await
    }

    pub async fn edit_dm(
        &self,
        actor_id: &str,
        conversation_id: &str,
        index: i64,
        body: &str,
    ) -> anyhow::Result<()> {
        self.state
            .edit_dm(actor_id, conversation_id, index, body)
            .await
    }

    pub async fn delete_dm(
        &self,
        actor_id: &str,
        conversation_id: &str,
        index: i64,
    ) -> anyhow::Result<()> {
        self.state.delete_dm(actor_id, conversation_id, index).await
    }

    pub async fn set_conversation_muted(
        &self,
        actor_id: &str,
        conversation_id: &str,
        ttl_hours: Option<i64>,
    ) -> anyhow::Result<()> {
        self.state
            .set_conversation_muted(actor_id, conversation_id, ttl_hours)
            .await
    }

    pub async fn set_conversation_saved(
        &self,
        actor_id: &str,
        conversation_id: &str,
        saved: bool,
    ) -> anyhow::Result<()> {
        self.state
            .set_conversation_saved(actor_id, conversation_id, saved)
            .await
    }

    pub async fn react_to_thread(
        &self,
        account_id: &str,
        thread_id: &str,
        emoji: &str,
        remove: bool,
    ) -> anyhow::Result<()> {
        self.state
            .react_to_thread(account_id, thread_id, emoji, remove)
            .await
    }

    pub async fn react_to_comment(
        &self,
        account_id: &str,
        thread_id: &str,
        index: i64,
        emoji: &str,
        remove: bool,
    ) -> anyhow::Result<()> {
        self.state
            .react_to_comment(account_id, thread_id, index, emoji, remove)
            .await
    }

    pub async fn react_to_dm(
        &self,
        account_id: &str,
        conversation_id: &str,
        index: i64,
        emoji: &str,
        remove: bool,
    ) -> anyhow::Result<()> {
        self.state
            .react_to_dm(account_id, conversation_id, index, emoji, remove)
            .await
    }

    pub async fn list_mentions(
        &self,
        account_id: &str,
        limit: i64,
    ) -> anyhow::Result<Vec<MentionSummary>> {
        self.state.list_mentions(account_id, limit).await
    }

    pub async fn list_notifications(
        &self,
        account_id: &str,
        limit: i64,
    ) -> anyhow::Result<Vec<NotificationSummary>> {
        self.state.list_notifications(account_id, limit).await
    }

    pub async fn terminal_notifications_enabled(&self, account_id: &str) -> anyhow::Result<bool> {
        self.state.terminal_notifications_enabled(account_id).await
    }

    pub async fn set_terminal_notifications(
        &self,
        account_id: &str,
        enabled: bool,
    ) -> anyhow::Result<()> {
        self.state
            .set_terminal_notifications(account_id, enabled)
            .await
    }

    pub async fn mark_notification_read(
        &self,
        account_id: &str,
        notification_id: Option<&str>,
    ) -> anyhow::Result<()> {
        self.state
            .mark_notification_read(account_id, notification_id)
            .await
    }

    pub async fn list_audit(&self, actor_id: &str, limit: i64) -> anyhow::Result<Vec<AuditEntry>> {
        self.state.list_audit(actor_id, limit).await
    }

    pub async fn search_page(
        &self,
        account_id: &str,
        query: &str,
        limit: i64,
    ) -> anyhow::Result<SearchPage> {
        self.state.search_page(account_id, query, limit).await
    }
}
