use crate::features::messages::model::NextUnread;

use super::ClientSession;

pub struct MessagesClient<'a> {
    session: &'a ClientSession,
}

impl ClientSession {
    pub fn messages(&self) -> MessagesClient<'_> {
        MessagesClient { session: self }
    }
}

impl MessagesClient<'_> {
    fn actor_id(&self) -> &str {
        self.session.actor_id()
    }

    pub async fn create_thread(&self, channel_id: String, title: String) -> anyhow::Result<String> {
        self.session
            .state()
            .create_thread(self.actor_id().to_string(), channel_id, title)
            .await
    }

    pub async fn add_comment(&self, thread_id: String, body: String) -> anyhow::Result<()> {
        self.session
            .state()
            .add_comment(self.actor_id().to_string(), thread_id, body)
            .await
    }

    pub async fn open_dm(&self, target: String) -> anyhow::Result<String> {
        self.session
            .state()
            .open_dm(self.actor_id().to_string(), target)
            .await
    }

    pub async fn send_dm(&self, conversation_id: String, body: String) -> anyhow::Result<()> {
        self.session
            .state()
            .send_dm(self.actor_id().to_string(), conversation_id, body)
            .await
    }

    pub async fn mark_thread_read(&self, thread_id: &str) -> anyhow::Result<()> {
        self.session
            .state()
            .mark_thread_read(self.actor_id(), thread_id)
            .await
    }

    pub async fn mark_thread_unread(&self, thread_id: &str) -> anyhow::Result<()> {
        self.session
            .state()
            .mark_thread_unread(self.actor_id(), thread_id)
            .await
    }

    pub async fn mark_conversation_read(&self, conversation_id: &str) -> anyhow::Result<()> {
        self.session
            .state()
            .mark_conversation_read(self.actor_id(), conversation_id)
            .await
    }

    pub async fn mark_conversation_unread(&self, conversation_id: &str) -> anyhow::Result<()> {
        self.session
            .state()
            .mark_conversation_unread(self.actor_id(), conversation_id)
            .await
    }

    pub async fn next_unread(&self) -> anyhow::Result<Option<NextUnread>> {
        self.session.state().next_unread(self.actor_id()).await
    }

    pub async fn rename_thread(&self, thread_id: &str, title: &str) -> anyhow::Result<()> {
        self.session
            .state()
            .rename_thread(self.actor_id(), thread_id, title)
            .await
    }

    pub async fn delete_thread(&self, thread_id: &str) -> anyhow::Result<()> {
        self.session
            .state()
            .delete_thread(self.actor_id(), thread_id)
            .await
    }

    pub async fn set_thread_archived(&self, thread_id: &str, archived: bool) -> anyhow::Result<()> {
        self.session
            .state()
            .set_thread_archived(self.actor_id(), thread_id, archived)
            .await
    }

    pub async fn set_thread_pinned(&self, thread_id: &str, pinned: bool) -> anyhow::Result<()> {
        self.session
            .state()
            .set_thread_pinned(self.actor_id(), thread_id, pinned)
            .await
    }

    pub async fn set_thread_muted(
        &self,
        thread_id: &str,
        ttl_hours: Option<i64>,
    ) -> anyhow::Result<()> {
        self.session
            .state()
            .set_thread_muted(self.actor_id(), thread_id, ttl_hours)
            .await
    }

    pub async fn set_thread_saved(&self, thread_id: &str, saved: bool) -> anyhow::Result<()> {
        self.session
            .state()
            .set_thread_saved(self.actor_id(), thread_id, saved)
            .await
    }

    pub async fn edit_comment(
        &self,
        thread_id: &str,
        index: i64,
        body: &str,
    ) -> anyhow::Result<()> {
        self.session
            .state()
            .edit_comment(self.actor_id(), thread_id, index, body)
            .await
    }

    pub async fn delete_comment(&self, thread_id: &str, index: i64) -> anyhow::Result<()> {
        self.session
            .state()
            .delete_comment(self.actor_id(), thread_id, index)
            .await
    }

    pub async fn edit_dm(
        &self,
        conversation_id: &str,
        index: i64,
        body: &str,
    ) -> anyhow::Result<()> {
        self.session
            .state()
            .edit_dm(self.actor_id(), conversation_id, index, body)
            .await
    }

    pub async fn delete_dm(&self, conversation_id: &str, index: i64) -> anyhow::Result<()> {
        self.session
            .state()
            .delete_dm(self.actor_id(), conversation_id, index)
            .await
    }

    pub async fn set_conversation_muted(
        &self,
        conversation_id: &str,
        ttl_hours: Option<i64>,
    ) -> anyhow::Result<()> {
        self.session
            .state()
            .set_conversation_muted(self.actor_id(), conversation_id, ttl_hours)
            .await
    }

    pub async fn set_comment_saved(
        &self,
        thread_id: &str,
        index: i64,
        saved: bool,
    ) -> anyhow::Result<()> {
        self.session
            .state()
            .set_comment_saved(self.actor_id(), thread_id, index, saved)
            .await
    }

    pub async fn set_dm_message_saved(
        &self,
        conversation_id: &str,
        index: i64,
        saved: bool,
    ) -> anyhow::Result<()> {
        self.session
            .state()
            .set_dm_message_saved(self.actor_id(), conversation_id, index, saved)
            .await
    }

    pub async fn react_to_thread(
        &self,
        thread_id: &str,
        emoji: &str,
        remove: bool,
    ) -> anyhow::Result<()> {
        self.session
            .state()
            .react_to_thread(self.actor_id(), thread_id, emoji, remove)
            .await
    }

    pub async fn react_to_comment(
        &self,
        thread_id: &str,
        index: i64,
        emoji: &str,
        remove: bool,
    ) -> anyhow::Result<()> {
        self.session
            .state()
            .react_to_comment(self.actor_id(), thread_id, index, emoji, remove)
            .await
    }

    pub async fn react_to_dm(
        &self,
        conversation_id: &str,
        index: i64,
        emoji: &str,
        remove: bool,
    ) -> anyhow::Result<()> {
        self.session
            .state()
            .react_to_dm(self.actor_id(), conversation_id, index, emoji, remove)
            .await
    }
}
