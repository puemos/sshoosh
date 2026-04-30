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
            ui.show_startup_splash(Duration::from_millis(2400));
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
            seen_notification_ids,
            pending_terminal_notifications: VecDeque::new(),
            emitted_terminal_title: None,
        })
    }

    pub async fn refresh(&mut self) -> anyhow::Result<()> {
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
        self.snapshot = self
            .client
            .snapshot(
                self.snapshot.selected_channel_id.as_deref(),
                self.snapshot.selected_thread_id.as_deref(),
                self.snapshot.selected_conversation_id.as_deref(),
                self.history_limit,
            )
            .await?;
        let terminal_notifications_enabled = self
            .client
            .terminal_notifications_enabled(&self.account.id)
            .await?;
        self.queue_new_terminal_notifications(terminal_notifications_enabled);
        if self.ui.route == Route::Search {
            self.snapshot.search_query = search_query;
            self.snapshot.search_results = search_results;
            self.snapshot.search_has_more = search_has_more;
            self.ui.search_selected = self
                .ui
                .search_selected
                .min(self.snapshot.search_results.len().saturating_sub(1));
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

    pub fn reset_search_limit(&mut self) -> i64 {
        self.search_limit = DEFAULT_SEARCH_LIMIT;
        self.search_limit
    }

    pub fn increase_search_limit(&mut self) -> i64 {
        self.search_limit = self
            .search_limit
            .saturating_add(SEARCH_PAGE_SIZE)
            .min(MAX_SEARCH_LIMIT);
        self.search_limit
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
        self.reset_detail_scroll();
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
        self.reset_detail_scroll();
        self.ui.route = Route::Channel(channel_id);
        self.ui.active_pane = ActivePane::Detail;
        self.ui.threads_collapsed = false;
        self.actions.push(Action::MarkThreadRead);
        self.refresh_requested = true;
    }

    pub fn select_thread_at_bottom(&mut self, channel_id: String, thread_id: String) {
        self.select_thread(channel_id, thread_id);
        self.scroll_detail_to_bottom();
    }

    pub fn select_conversation(&mut self, conversation_id: String) {
        self.reset_history_limit();
        self.snapshot.selected_conversation_id = Some(conversation_id);
        self.reset_detail_scroll();
        self.ui.route = Route::Dms;
        self.ui.active_pane = ActivePane::Detail;
        self.actions.push(Action::MarkDmRead);
        self.refresh_requested = true;
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
        self.snapshot.selected_conversation_id = None;
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
