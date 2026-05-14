use crate::features::channels::model::{ChannelDirectoryItem, ChannelMemberSummary};

use super::ClientSession;

pub struct ChannelsClient<'a> {
    session: &'a ClientSession,
}

impl ClientSession {
    pub fn channels(&self) -> ChannelsClient<'_> {
        ChannelsClient { session: self }
    }
}

impl ChannelsClient<'_> {
    fn actor_id(&self) -> &str {
        self.session.actor_id()
    }

    pub async fn create_channel(&self, name: String, private: bool) -> anyhow::Result<String> {
        self.session
            .state()
            .create_channel(self.actor_id().to_string(), name, private)
            .await
    }

    pub async fn join_channel(&self, slug: String) -> anyhow::Result<String> {
        self.session
            .state()
            .join_channel(self.actor_id().to_string(), slug)
            .await
    }

    pub async fn leave_channel(&self, slug: &str) -> anyhow::Result<()> {
        self.session
            .state()
            .leave_channel(self.actor_id(), slug)
            .await
    }

    pub async fn list_channels(
        &self,
        include_archived: bool,
    ) -> anyhow::Result<Vec<ChannelDirectoryItem>> {
        self.session
            .state()
            .list_channels(self.actor_id(), include_archived)
            .await
    }

    pub async fn rename_channel(&self, slug: &str, name: &str) -> anyhow::Result<()> {
        self.session
            .state()
            .rename_channel(self.actor_id(), slug, name)
            .await
    }

    pub async fn set_channel_topic(&self, slug: &str, topic: &str) -> anyhow::Result<()> {
        self.session
            .state()
            .set_channel_topic(self.actor_id(), slug, topic)
            .await
    }

    pub async fn set_channel_archived(&self, slug: &str, archived: bool) -> anyhow::Result<()> {
        self.session
            .state()
            .set_channel_archived(self.actor_id(), slug, archived)
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

    pub async fn add_channel_member(&self, slug: &str, username: &str) -> anyhow::Result<()> {
        self.session
            .state()
            .add_channel_member(self.actor_id(), slug, username)
            .await
    }

    pub async fn remove_channel_member(&self, slug: &str, username: &str) -> anyhow::Result<()> {
        self.session
            .state()
            .remove_channel_member(self.actor_id(), slug, username)
            .await
    }
}
