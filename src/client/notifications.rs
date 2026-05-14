use crate::features::{
    feeds::model::{Page, PageRequest},
    notifications::model::*,
};

use super::ClientSession;

pub struct NotificationsClient<'a> {
    session: &'a ClientSession,
}

impl ClientSession {
    pub fn notifications(&self) -> NotificationsClient<'_> {
        NotificationsClient { session: self }
    }
}

impl NotificationsClient<'_> {
    fn actor_id(&self) -> &str {
        self.session.actor_id()
    }

    pub async fn list_mentions(&self, limit: i64) -> anyhow::Result<Vec<MentionSummary>> {
        self.session
            .state()
            .list_mentions(self.actor_id(), limit)
            .await
    }

    pub async fn list_notifications(&self, limit: i64) -> anyhow::Result<Vec<NotificationSummary>> {
        self.session
            .state()
            .list_notifications(self.actor_id(), limit)
            .await
    }

    pub async fn list_notifications_page(
        &self,
        request: PageRequest,
    ) -> anyhow::Result<Page<NotificationSummary>> {
        self.session
            .state()
            .list_notifications_page(self.actor_id(), request)
            .await
    }

    pub async fn terminal_notifications_enabled(&self) -> anyhow::Result<bool> {
        self.session
            .state()
            .terminal_notifications_enabled(self.actor_id())
            .await
    }

    pub async fn set_terminal_notifications(&self, enabled: bool) -> anyhow::Result<()> {
        self.session
            .state()
            .set_terminal_notifications(self.actor_id(), enabled)
            .await
    }

    pub async fn mark_notification_read(
        &self,
        notification_id: Option<&str>,
    ) -> anyhow::Result<()> {
        self.session
            .state()
            .mark_notification_read(self.actor_id(), notification_id)
            .await
    }

    pub async fn archive_notifications(&self) -> anyhow::Result<()> {
        self.session
            .state()
            .archive_notifications(self.actor_id())
            .await
    }
}
