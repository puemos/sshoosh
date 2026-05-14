use crate::features::{
    feeds::model::{Page, PageRequest, SearchPage},
    messages::model::{HotLabel, LabelFeedItem, SavedMessageItem},
};

use super::ClientSession;

pub struct FeedsClient<'a> {
    session: &'a ClientSession,
}

impl ClientSession {
    pub fn feeds(&self) -> FeedsClient<'_> {
        FeedsClient { session: self }
    }
}

impl FeedsClient<'_> {
    fn actor_id(&self) -> &str {
        self.session.actor_id()
    }

    pub async fn search_page(&self, query: &str, limit: i64) -> anyhow::Result<SearchPage> {
        self.session
            .state()
            .search_page(self.actor_id(), query, limit)
            .await
    }

    pub async fn search_page_after(
        &self,
        query: &str,
        request: PageRequest,
    ) -> anyhow::Result<SearchPage> {
        self.session
            .state()
            .search_page_after(self.actor_id(), query, request)
            .await
    }

    pub async fn saved_messages_page(
        &self,
        limit: i64,
    ) -> anyhow::Result<(Vec<SavedMessageItem>, bool)> {
        self.session
            .state()
            .saved_messages_page(self.actor_id(), limit)
            .await
    }

    pub async fn saved_messages_page_after(
        &self,
        request: PageRequest,
    ) -> anyhow::Result<Page<SavedMessageItem>> {
        self.session
            .state()
            .saved_messages_page_after(self.actor_id(), request)
            .await
    }

    pub async fn hot_labels(&self, limit: i64) -> anyhow::Result<Vec<HotLabel>> {
        self.session
            .state()
            .hot_labels(self.actor_id(), limit)
            .await
    }

    pub async fn label_feed_page_after(
        &self,
        tag: &str,
        request: PageRequest,
    ) -> anyhow::Result<Page<LabelFeedItem>> {
        self.session
            .state()
            .label_feed_page_after(self.actor_id(), tag, request)
            .await
    }
}
