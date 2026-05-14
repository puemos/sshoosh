use crate::features::{
    accounts::model::{Account, AccountSummary, InviteSummary, Role, SshKeySummary},
    channels::model::ChannelMemberSummary,
};

use super::ClientSession;

pub struct AccountsClient<'a> {
    session: &'a ClientSession,
}

impl ClientSession {
    pub fn accounts(&self) -> AccountsClient<'_> {
        AccountsClient { session: self }
    }
}

impl AccountsClient<'_> {
    fn actor_id(&self) -> &str {
        self.session.actor_id()
    }

    pub async fn create_invite(&self) -> anyhow::Result<String> {
        self.session
            .state()
            .create_invite(self.actor_id().to_string())
            .await
    }

    pub async fn create_invite_with_options(
        &self,
        role: Role,
        ttl_hours: Option<i64>,
    ) -> anyhow::Result<String> {
        self.session
            .state()
            .create_invite_with_options(self.actor_id(), role, ttl_hours)
            .await
    }

    pub async fn complete_onboarding(&self, username: &str) -> anyhow::Result<Account> {
        self.session
            .state()
            .complete_onboarding(self.actor_id(), username)
            .await
    }

    pub async fn list_accounts(&self) -> anyhow::Result<Vec<AccountSummary>> {
        self.session.state().list_accounts(self.actor_id()).await
    }

    pub async fn rename_self(&self, username: &str) -> anyhow::Result<()> {
        self.session
            .state()
            .rename_user(self.actor_id(), self.actor_id(), username)
            .await
    }

    pub async fn set_self_display_name(&self, display_name: &str) -> anyhow::Result<()> {
        self.session
            .state()
            .set_display_name(self.actor_id(), self.actor_id(), display_name)
            .await
    }

    pub async fn set_user_disabled(&self, username: &str, disabled: bool) -> anyhow::Result<()> {
        self.session
            .state()
            .set_user_disabled(self.actor_id(), username, disabled)
            .await
    }

    pub async fn set_user_role(&self, username: &str, role: Role) -> anyhow::Result<()> {
        self.session
            .state()
            .set_user_role(self.actor_id(), username, role)
            .await
    }

    pub async fn list_ssh_keys(&self) -> anyhow::Result<Vec<SshKeySummary>> {
        self.session.state().list_ssh_keys(self.actor_id()).await
    }

    pub async fn list_my_ssh_keys(&self) -> anyhow::Result<Vec<SshKeySummary>> {
        self.session.state().list_my_ssh_keys(self.actor_id()).await
    }

    pub async fn create_device_link_token(&self, label: Option<&str>) -> anyhow::Result<String> {
        self.session
            .state()
            .create_device_link_token(self.actor_id(), label)
            .await
    }

    pub async fn add_ssh_key(
        &self,
        username: Option<&str>,
        public_key: &str,
        label: Option<&str>,
    ) -> anyhow::Result<SshKeySummary> {
        self.session
            .state()
            .add_ssh_key(self.actor_id(), username, public_key, label)
            .await
    }

    pub async fn label_ssh_key(&self, key: &str, label: &str) -> anyhow::Result<()> {
        self.session
            .state()
            .label_ssh_key(self.actor_id(), key, label)
            .await
    }

    pub async fn revoke_ssh_key(&self, key: &str) -> anyhow::Result<()> {
        self.session
            .state()
            .revoke_ssh_key(self.actor_id(), key)
            .await
    }

    pub async fn list_invites(&self) -> anyhow::Result<Vec<InviteSummary>> {
        self.session.state().list_invites(self.actor_id()).await
    }

    pub async fn revoke_invite(&self, invite_id: &str) -> anyhow::Result<()> {
        self.session
            .state()
            .revoke_invite(self.actor_id(), invite_id)
            .await
    }

    pub async fn list_channel_members(
        &self,
        slug: &str,
    ) -> anyhow::Result<Vec<ChannelMemberSummary>> {
        self.session
            .state()
            .list_channel_members(self.actor_id(), slug)
            .await
    }
}
