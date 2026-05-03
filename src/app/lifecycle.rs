use super::*;
impl App {
    pub async fn new(
        account: Account,
        state: ServerState,
        cols: u16,
        rows: u16,
    ) -> anyhow::Result<Self> {
        let (terminal, shared) = terminal::terminal(cols.max(80), rows.max(24))?;
        let client = ClientSession::new(account, state);
        let live_rx = client.subscribe();
        let snapshot = client
            .snapshot(None, None, None, DEFAULT_HISTORY_LIMIT)
            .await?;
        let seen_notification_ids = notification_ids(&snapshot.notifications);
        let account = client.account().clone();
        let mut ui = UiState::default();
        ui.sync_route_from_snapshot(&snapshot);
        if account.activated {
            ui.route = Route::Notifications;
            ui.active_pane = ActivePane::Detail;
        }
        if account.activated {
            ui.show_startup_splash(Duration::from_millis(2400));
        } else if let Some(username) = account.pending_username.as_deref() {
            ui.composer.start(username);
        }
        Ok(Self {
            running: true,
            terminal,
            shared,
            account,
            client,
            live_rx,
            snapshot,
            ui,
            commands: CommandRegistry::default(),
            decoder: InputDecoder::default(),
            actions: Vec::new(),
            refresh_requested: false,
            pending_link_open: None,
            pending_clipboard_copy: None,
            desired_pointer_shape: PointerShape::Default,
            emitted_pointer_shape: PointerShape::Default,
            history_limit: DEFAULT_HISTORY_LIMIT,
            search_limit: DEFAULT_SEARCH_LIMIT,
            saved_limit: DEFAULT_SEARCH_LIMIT,
            label_limit: DEFAULT_SEARCH_LIMIT,
            seen_notification_ids,
            pending_terminal_notifications: VecDeque::new(),
            emitted_terminal_title: None,
        })
    }

    pub async fn refresh(&mut self) -> anyhow::Result<()> {
        let was_activated = self.account.activated;
        self.account = match self.client.refresh_account().await {
            Ok(account) => account,
            Err(err) => {
                self.running = false;
                return Err(err);
            }
        };
        let search_query = self.snapshot.search_query.clone();
        let search_results = self.snapshot.search_results.clone();
        let search_has_more = self.snapshot.search_has_more;
        let search_next_cursor = self.snapshot.search_next_cursor.clone();
        let saved_messages = self.snapshot.saved_messages.clone();
        let saved_has_more = self.snapshot.saved_has_more;
        let saved_next_cursor = self.snapshot.saved_next_cursor.clone();
        let label_query = self.snapshot.label_query.clone();
        let label_items = self.snapshot.label_items.clone();
        let label_has_more = self.snapshot.label_has_more;
        let label_next_cursor = self.snapshot.label_next_cursor.clone();
        let notifications = self.snapshot.notifications.clone();
        let notifications_next_cursor = self.snapshot.notifications_next_cursor.clone();
        self.snapshot = self
            .client
            .snapshot(
                self.snapshot.selected_channel_id.as_deref(),
                self.snapshot.selected_thread_id.as_deref(),
                self.snapshot.selected_conversation_id.as_deref(),
                self.history_limit,
            )
            .await?;
        if !was_activated && self.account.activated {
            self.ui.route = Route::Notifications;
            self.ui.active_pane = ActivePane::Detail;
            self.ui.composer.reset_input();
            self.ui.show_startup_splash(Duration::from_millis(2400));
        }
        let terminal_notifications_enabled = self
            .client
            .terminal_notifications_enabled(&self.account.id)
            .await?;
        self.queue_new_terminal_notifications(terminal_notifications_enabled);
        if self.ui.route == Route::Search {
            self.snapshot.search_query = search_query;
            self.snapshot.search_results = search_results;
            self.snapshot.search_has_more = search_has_more;
            self.snapshot.search_next_cursor = search_next_cursor;
            self.ui.search_selected = self
                .ui
                .search_selected
                .min(self.snapshot.search_results.len().saturating_sub(1));
        } else if self.ui.route == Route::Saved {
            self.snapshot.saved_messages = saved_messages;
            self.snapshot.saved_has_more = saved_has_more;
            self.snapshot.saved_next_cursor = saved_next_cursor;
            self.ui.saved_selected = self
                .ui
                .saved_selected
                .min(self.snapshot.saved_messages.len().saturating_sub(1));
        } else if matches!(self.ui.route, Route::Label(_)) {
            self.snapshot.label_query = label_query;
            self.snapshot.label_items = label_items;
            self.snapshot.label_has_more = label_has_more;
            self.snapshot.label_next_cursor = label_next_cursor;
            self.ui.label_selected = self
                .ui
                .label_selected
                .min(self.snapshot.label_items.len().saturating_sub(1));
        } else if self.ui.route == Route::Notifications {
            self.snapshot.notifications = notifications;
            self.snapshot.notifications_next_cursor = notifications_next_cursor;
            let visible_len = self.visible_notification_indices().len();
            self.ui.notifications_selected = self
                .ui
                .notifications_selected
                .min(visible_len.saturating_sub(1));
        }
        self.ui.sync_route_from_snapshot(&self.snapshot);
        self.update_completions();
        self.refresh_requested = false;
        Ok(())
    }

    pub fn drain_live_events(&mut self) -> bool {
        let mut changed = false;
        loop {
            match self.live_rx.try_recv() {
                Ok(_) => changed = true,
                Err(tokio::sync::broadcast::error::TryRecvError::Empty) => break,
                Err(tokio::sync::broadcast::error::TryRecvError::Lagged(_)) => {
                    changed = true;
                    break;
                }
                Err(tokio::sync::broadcast::error::TryRecvError::Closed) => break,
            }
        }
        changed
    }

    pub fn take_actions(&mut self) -> Vec<Action> {
        std::mem::take(&mut self.actions)
    }

    pub fn take_refresh_requested(&mut self) -> bool {
        std::mem::take(&mut self.refresh_requested)
    }

    pub(crate) fn client_session(&self) -> ClientSession {
        self.client.clone()
    }

    pub fn set_banner_ok(&mut self, text: impl Into<String>) {
        self.ui.banner = Some(Banner::ok(text));
    }

    pub fn set_banner_modal_ok(&mut self, text: impl Into<String>) {
        self.ui.banner = Some(Banner::modal_ok(text));
    }

    pub fn set_banner_list(&mut self, list: super::ListModal) {
        self.ui.banner = Some(Banner::list(list));
    }

    pub fn set_banner_err(&mut self, text: impl Into<String>) {
        self.ui.banner = Some(Banner::err(text));
    }

    pub fn selected_channel_id(&self) -> Option<String> {
        self.snapshot.selected_channel_id.clone()
    }

    pub fn selected_channel_slug(&self) -> Option<String> {
        self.snapshot
            .selected_channel_id
            .as_ref()
            .and_then(|id| {
                self.snapshot
                    .channels
                    .iter()
                    .find(|channel| &channel.id == id)
            })
            .map(|channel| channel.slug.clone())
    }

    pub fn has_channel(&self, channel_id: &str) -> bool {
        self.snapshot
            .channels
            .iter()
            .any(|channel| channel.id == channel_id)
    }

    pub fn selected_thread_id(&self) -> Option<String> {
        self.snapshot.selected_thread_id.clone()
    }

    pub fn selected_conversation_id(&self) -> Option<String> {
        self.snapshot.selected_conversation_id.clone()
    }

    pub(crate) fn terminal_title_update(&mut self) -> Option<Vec<u8>> {
        let title = self.current_terminal_title();
        if self.emitted_terminal_title.as_deref() == Some(title.as_str()) {
            None
        } else {
            self.emitted_terminal_title = Some(title.clone());
            Some(terminal::terminal_title(&title))
        }
    }

    pub fn search_query(&self) -> Option<String> {
        matches!(self.ui.route, Route::Search)
            .then(|| self.snapshot.search_query.clone())
            .flatten()
    }

    pub fn saved_active(&self) -> bool {
        self.ui.route == Route::Saved
    }

    pub fn label_query(&self) -> Option<String> {
        matches!(self.ui.route, Route::Label(_))
            .then(|| self.snapshot.label_query.clone())
            .flatten()
    }

    pub fn notifications_active(&self) -> bool {
        self.ui.route == Route::Notifications
    }

    pub fn search_page_request(&self) -> Option<(String, Option<String>)> {
        self.search_query()
            .map(|query| (query, self.snapshot.search_next_cursor.clone()))
    }

    pub fn saved_next_cursor(&self) -> Option<String> {
        self.saved_active()
            .then(|| self.snapshot.saved_next_cursor.clone())
            .flatten()
    }

    pub fn label_page_request(&self) -> Option<(String, Option<String>)> {
        self.label_query()
            .map(|tag| (tag, self.snapshot.label_next_cursor.clone()))
    }

    pub fn notifications_next_cursor(&self) -> Option<String> {
        self.notifications_active()
            .then(|| self.snapshot.notifications_next_cursor.clone())
            .flatten()
    }

    pub fn reset_search_limit(&mut self) -> i64 {
        self.search_limit = DEFAULT_SEARCH_LIMIT;
        self.search_limit
    }

    pub fn reset_saved_limit(&mut self) -> i64 {
        self.saved_limit = DEFAULT_SEARCH_LIMIT;
        self.saved_limit
    }

    pub fn reset_label_limit(&mut self) -> i64 {
        self.label_limit = DEFAULT_SEARCH_LIMIT;
        self.label_limit
    }

    pub fn increase_history_limit(&mut self) -> i64 {
        self.history_limit = self
            .history_limit
            .saturating_add(DEFAULT_HISTORY_LIMIT)
            .min(MAX_HISTORY_LIMIT);
        self.history_limit
    }

    pub fn select_channel(&mut self, channel_id: String) {
        self.reset_history_limit();
        self.snapshot.selected_channel_id = Some(channel_id.clone());
        self.snapshot.selected_thread_id = None;
        self.snapshot.selected_conversation_id = None;
        self.snapshot.threads.clear();
        self.snapshot.comments.clear();
        self.ui.pending_source_focus = None;
        self.reset_detail_scroll_to_bottom();
        self.ui.route = Route::Channel(channel_id);
        self.ui.active_pane = ActivePane::List;
        self.ui.threads_collapsed = false;
        self.refresh_requested = true;
    }

    pub fn select_thread(&mut self, channel_id: String, thread_id: String) {
        self.reset_history_limit();
        self.snapshot.selected_channel_id = Some(channel_id.clone());
        self.snapshot.selected_thread_id = Some(thread_id);
        self.snapshot.selected_conversation_id = None;
        self.ui.pending_source_focus = None;
        self.reset_detail_scroll_to_bottom();
        self.ui.route = Route::Channel(channel_id);
        self.ui.active_pane = ActivePane::Detail;
        self.ui.threads_collapsed = false;
        self.actions.push(Action::MarkThreadRead);
        self.refresh_requested = true;
    }

    pub fn select_thread_with_focus(
        &mut self,
        channel_id: String,
        thread_id: String,
        focus: SourceFocus,
    ) {
        self.select_thread(channel_id, thread_id);
        self.prepare_source_focus(focus);
    }

    pub fn select_thread_at_bottom(&mut self, channel_id: String, thread_id: String) {
        self.select_thread(channel_id, thread_id);
        self.scroll_detail_to_bottom();
    }

    pub fn select_conversation(&mut self, conversation_id: String) {
        self.reset_history_limit();
        self.snapshot.selected_conversation_id = Some(conversation_id);
        self.ui.pending_source_focus = None;
        self.reset_detail_scroll();
        self.ui.route = Route::Dms;
        self.ui.active_pane = ActivePane::Detail;
        self.actions.push(Action::MarkDmRead);
        self.refresh_requested = true;
    }

    pub fn select_conversation_with_focus(&mut self, conversation_id: String, focus: SourceFocus) {
        self.select_conversation(conversation_id);
        self.prepare_source_focus(focus);
    }

    fn prepare_source_focus(&mut self, focus: SourceFocus) {
        if matches!(focus, SourceFocus::Comment(_) | SourceFocus::Dm(_)) {
            self.history_limit = MAX_HISTORY_LIMIT;
        }
        self.ui.pending_source_focus = Some(focus);
    }

    pub fn select_conversation_at_bottom(&mut self, conversation_id: String) {
        self.select_conversation(conversation_id);
        self.scroll_detail_to_bottom();
    }

    pub fn set_search_results(
        &mut self,
        query: String,
        results: Vec<SearchResult>,
        has_more: bool,
        reset_selection: bool,
    ) {
        self.snapshot.search_query = Some(query);
        self.snapshot.search_results = results;
        self.snapshot.search_has_more = has_more;
        if !has_more {
            self.snapshot.search_next_cursor = None;
        }
        self.snapshot.selected_conversation_id = None;
        self.ui.pending_source_focus = None;
        self.ui.route = Route::Search;
        self.ui.active_pane = ActivePane::Detail;
        if reset_selection {
            self.ui.search_selected = 0;
            self.reset_detail_scroll();
        } else {
            self.ui.search_selected = self
                .ui
                .search_selected
                .min(self.snapshot.search_results.len().saturating_sub(1));
        }
    }

    pub fn set_saved_messages(
        &mut self,
        messages: Vec<SavedMessageItem>,
        has_more: bool,
        reset_selection: bool,
    ) {
        self.snapshot.saved_messages = messages;
        self.snapshot.saved_has_more = has_more;
        if !has_more {
            self.snapshot.saved_next_cursor = None;
        }
        self.snapshot.selected_conversation_id = None;
        self.ui.pending_source_focus = None;
        self.ui.route = Route::Saved;
        self.ui.active_pane = ActivePane::Detail;
        if reset_selection {
            self.ui.saved_selected = 0;
            self.reset_detail_scroll();
        } else {
            self.ui.saved_selected = self
                .ui
                .saved_selected
                .min(self.snapshot.saved_messages.len().saturating_sub(1));
        }
    }

    pub fn set_label_feed(
        &mut self,
        tag: String,
        items: Vec<LabelFeedItem>,
        has_more: bool,
        reset_selection: bool,
    ) {
        self.snapshot.label_query = Some(tag.clone());
        self.snapshot.label_items = items;
        self.snapshot.label_has_more = has_more;
        if !has_more {
            self.snapshot.label_next_cursor = None;
        }
        self.snapshot.selected_thread_id = None;
        self.snapshot.selected_conversation_id = None;
        self.ui.pending_source_focus = None;
        self.ui.route = Route::Label(tag);
        self.ui.active_pane = ActivePane::Detail;
        if reset_selection {
            self.ui.label_selected = 0;
            self.reset_detail_scroll();
        } else {
            self.ui.label_selected = self
                .ui
                .label_selected
                .min(self.snapshot.label_items.len().saturating_sub(1));
        }
    }

    pub fn set_search_results_page(
        &mut self,
        query: String,
        results: Vec<SearchResult>,
        next_cursor: Option<String>,
        reset_selection: bool,
    ) {
        let has_more = next_cursor.is_some();
        self.set_search_results(query, results, has_more, reset_selection);
        self.snapshot.search_next_cursor = next_cursor;
    }

    pub fn append_search_results(
        &mut self,
        query: String,
        mut results: Vec<SearchResult>,
        next_cursor: Option<String>,
    ) {
        if self.snapshot.search_query.as_deref() == Some(query.as_str()) {
            self.snapshot.search_results.append(&mut results);
        } else {
            self.snapshot.search_query = Some(query);
            self.snapshot.search_results = results;
            self.ui.search_selected = 0;
        }
        self.snapshot.search_has_more = next_cursor.is_some();
        self.snapshot.search_next_cursor = next_cursor;
        self.snapshot.selected_conversation_id = None;
        self.ui.pending_source_focus = None;
        self.ui.route = Route::Search;
        self.ui.active_pane = ActivePane::Detail;
    }

    pub fn set_saved_messages_page(
        &mut self,
        messages: Vec<SavedMessageItem>,
        next_cursor: Option<String>,
        reset_selection: bool,
    ) {
        let has_more = next_cursor.is_some();
        self.set_saved_messages(messages, has_more, reset_selection);
        self.snapshot.saved_next_cursor = next_cursor;
    }

    pub fn set_label_feed_page(
        &mut self,
        tag: String,
        items: Vec<LabelFeedItem>,
        next_cursor: Option<String>,
        reset_selection: bool,
    ) {
        let has_more = next_cursor.is_some();
        self.set_label_feed(tag, items, has_more, reset_selection);
        self.snapshot.label_next_cursor = next_cursor;
    }

    pub fn append_saved_messages(
        &mut self,
        mut messages: Vec<SavedMessageItem>,
        next_cursor: Option<String>,
    ) {
        self.snapshot.saved_messages.append(&mut messages);
        self.snapshot.saved_has_more = next_cursor.is_some();
        self.snapshot.saved_next_cursor = next_cursor;
        self.snapshot.selected_conversation_id = None;
        self.ui.pending_source_focus = None;
        self.ui.route = Route::Saved;
        self.ui.active_pane = ActivePane::Detail;
    }

    pub fn append_label_feed(
        &mut self,
        tag: String,
        mut items: Vec<LabelFeedItem>,
        next_cursor: Option<String>,
    ) {
        if self.snapshot.label_query.as_deref() == Some(tag.as_str()) {
            self.snapshot.label_items.append(&mut items);
        } else {
            self.snapshot.label_query = Some(tag.clone());
            self.snapshot.label_items = items;
            self.ui.label_selected = 0;
        }
        self.snapshot.label_has_more = next_cursor.is_some();
        self.snapshot.label_next_cursor = next_cursor;
        self.snapshot.selected_thread_id = None;
        self.snapshot.selected_conversation_id = None;
        self.ui.pending_source_focus = None;
        self.ui.route = Route::Label(tag);
        self.ui.active_pane = ActivePane::Detail;
    }

    pub fn set_notifications_page(
        &mut self,
        notifications: Vec<NotificationSummary>,
        next_cursor: Option<String>,
        reset_selection: bool,
    ) {
        self.snapshot.notifications = notifications;
        self.snapshot.notifications_next_cursor = next_cursor;
        self.snapshot.selected_thread_id = None;
        self.snapshot.selected_conversation_id = None;
        self.ui.pending_source_focus = None;
        self.ui.route = Route::Notifications;
        self.ui.active_pane = ActivePane::Detail;
        if reset_selection {
            self.ui.notifications_selected = 0;
            self.reset_detail_scroll();
        } else {
            let visible_len = self.visible_notification_indices().len();
            self.ui.notifications_selected = self
                .ui
                .notifications_selected
                .min(visible_len.saturating_sub(1));
        }
    }

    pub fn append_notifications(
        &mut self,
        mut notifications: Vec<NotificationSummary>,
        next_cursor: Option<String>,
    ) {
        self.snapshot.notifications.append(&mut notifications);
        self.snapshot.notifications_next_cursor = next_cursor;
        self.ui.route = Route::Notifications;
        self.ui.active_pane = ActivePane::Detail;
    }

    fn queue_new_terminal_notifications(&mut self, enabled: bool) {
        for notification in &self.snapshot.notifications {
            if notification.read_at.is_none()
                && enabled
                && !self.seen_notification_ids.contains(&notification.id)
            {
                self.pending_terminal_notifications
                    .push_back(TerminalNotification {
                        id: notification.id.clone(),
                        title: notification.title.clone(),
                        body: notification.body.clone(),
                    });
            }
            self.seen_notification_ids.insert(notification.id.clone());
        }
    }

    fn current_terminal_title(&self) -> String {
        let Some(slug) = self.selected_channel_slug() else {
            return "sshoosh".to_string();
        };
        let mut title = format!("sshoosh • #{slug}");
        if self.snapshot.notification_unread_count > 0 {
            title.push_str(&format!(
                " • {} unread",
                self.snapshot.notification_unread_count
            ));
        }
        title
    }
}

fn notification_ids(notifications: &[NotificationSummary]) -> HashSet<String> {
    notifications
        .iter()
        .map(|notification| notification.id.clone())
        .collect()
}
