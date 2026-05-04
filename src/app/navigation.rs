use super::*;
impl App {
    pub(crate) fn move_selection(&mut self, delta: isize) {
        if self.ui.route == Route::Search {
            self.move_search(delta);
            return;
        }
        if matches!(self.ui.route, Route::Label(_)) && self.ui.active_pane == ActivePane::Detail {
            self.move_label(delta);
            return;
        }
        if self.ui.route == Route::Saved && self.ui.active_pane == ActivePane::Detail {
            self.move_saved(delta);
            return;
        }
        if self.ui.route == Route::Notifications && self.ui.active_pane == ActivePane::Detail {
            self.move_notifications(delta);
            return;
        }
        if self.ui.active_pane == ActivePane::Detail {
            self.move_detail(delta);
        } else {
            self.move_workspace(delta);
        }
    }

    pub(crate) fn move_to_edge(&mut self, end: bool) {
        if self.ui.active_pane == ActivePane::Detail {
            if end {
                self.ui.detail_scroll.scroll_to_bottom();
                self.queue_result_list_page_if_available();
            } else if self.detail_at_top() {
                self.queue_older_history_if_available();
            } else {
                self.ui.detail_scroll.scroll_to_top();
            }
        } else {
            let delta = if end { isize::MAX / 2 } else { isize::MIN / 2 };
            self.move_selection(delta);
        }
    }

    pub(crate) fn move_detail(&mut self, delta: isize) {
        if delta < 0 {
            if self.detail_at_top() {
                self.queue_older_history_if_available();
            }
            for _ in 0..delta.unsigned_abs() {
                self.ui.detail_scroll.scroll_up();
            }
        } else {
            if self.detail_at_bottom() {
                self.queue_result_list_page_if_available();
            }
            for _ in 0..delta as usize {
                self.ui.detail_scroll.scroll_down();
            }
        }
    }

    pub(crate) fn page_detail(&mut self, down: bool) {
        let step = self
            .ui
            .detail_scroll_metrics
            .viewport_height
            .saturating_sub(1)
            .max(1);
        let offset = self.ui.detail_scroll.offset();
        let max_y = self.ui.detail_scroll_metrics.max_y_offset;
        if down {
            if self.detail_at_bottom() {
                self.queue_result_list_page_if_available();
            }
            self.ui.detail_scroll.set_offset(Position {
                x: 0,
                y: offset.y.saturating_add(step).min(max_y),
            });
        } else {
            if self.detail_at_top() {
                self.queue_older_history_if_available();
            }
            self.ui.detail_scroll.set_offset(Position {
                x: 0,
                y: offset.y.saturating_sub(step),
            });
        }
    }

    pub(crate) fn move_search(&mut self, delta: isize) {
        let len = self.snapshot.search_results.len();
        if len == 0 {
            return;
        }
        self.ui.search_selected = clamp_index(self.ui.search_selected, delta, len);
    }

    pub(crate) fn move_label(&mut self, delta: isize) {
        let len = self.snapshot.label_items.len();
        if len == 0 {
            return;
        }
        let next = clamp_index(self.ui.label_selected, delta, len);
        if next != self.ui.label_selected {
            self.ui.label_selected = next;
            self.ui.detail_selection_scroll_pending = true;
        } else if delta > 0 && self.ui.label_selected == len.saturating_sub(1) {
            self.queue_result_list_page_if_available();
        }
    }

    pub(crate) fn move_saved(&mut self, delta: isize) {
        let len = self.snapshot.saved_messages.len();
        if len == 0 {
            return;
        }
        let next = clamp_index(self.ui.saved_selected, delta, len);
        if next != self.ui.saved_selected {
            self.ui.saved_selected = next;
            self.ui.detail_selection_scroll_pending = true;
        } else if delta > 0 && self.ui.saved_selected == len.saturating_sub(1) {
            self.queue_result_list_page_if_available();
        }
    }

    pub(crate) fn move_notifications(&mut self, delta: isize) {
        let len = self.visible_notification_indices().len();
        if len == 0 {
            return;
        }
        let next = clamp_index(self.ui.notifications_selected, delta, len);
        if next != self.ui.notifications_selected {
            self.ui.notifications_selected = next;
            self.ui.detail_selection_scroll_pending = true;
        } else if delta > 0 && self.ui.notifications_selected == len.saturating_sub(1) {
            self.queue_result_list_page_if_available();
        }
    }

    pub(crate) fn can_load_older_history(&self) -> bool {
        self.history_limit < MAX_HISTORY_LIMIT
            && match self.ui.route {
                Route::Channel(_) => {
                    self.snapshot.selected_thread_id.is_some() && self.snapshot.comments_has_more
                }
                Route::Dms => {
                    self.snapshot.selected_conversation_id.is_some()
                        && self.snapshot.conversation_messages_has_more
                }
                Route::Search | Route::Label(_) | Route::Saved | Route::Notifications => false,
            }
    }

    pub(crate) fn prepare_older_history_anchor(&mut self) {
        if let Some(focus) = self.first_visible_message_focus() {
            self.ui.pending_source_focus = Some(focus);
            return;
        }
        self.ui.pending_source_focus = match self.ui.route {
            Route::Channel(_) => self
                .snapshot
                .comments
                .first()
                .map(|comment| SourceFocus::Comment(comment.obj_index))
                .or(Some(SourceFocus::ThreadRoot)),
            Route::Dms => self
                .snapshot
                .conversation_messages
                .first()
                .map(|message| SourceFocus::Dm(message.obj_index)),
            Route::Search | Route::Label(_) | Route::Saved | Route::Notifications => None,
        };
    }

    fn first_visible_message_focus(&self) -> Option<SourceFocus> {
        let viewport_height = self.ui.detail_scroll_metrics.viewport_height.max(1);
        let top = (0..u16::MAX).find(|row| {
            self.ui
                .hit_map
                .hit_row_matching(*row, |target| matches!(target, HitTarget::DetailScroll))
                .is_some()
        })?;
        let bottom = top.saturating_add(viewport_height);
        for row in top..bottom {
            let Some(region) = self.ui.hit_map.hit_row_matching(row, |target| {
                matches!(target, HitTarget::EditableMessage(_))
            }) else {
                continue;
            };
            match region.target {
                HitTarget::EditableMessage(EditableMessageTarget::Comment(index))
                    if matches!(self.ui.route, Route::Channel(_)) =>
                {
                    return Some(SourceFocus::Comment(index));
                }
                HitTarget::EditableMessage(EditableMessageTarget::Dm(index))
                    if matches!(self.ui.route, Route::Dms) =>
                {
                    return Some(SourceFocus::Dm(index));
                }
                _ => {}
            }
        }
        None
    }

    fn can_load_more_result_list(&self) -> bool {
        match self.ui.route {
            Route::Label(_) => self.snapshot.label_next_cursor.is_some(),
            Route::Saved => self.snapshot.saved_next_cursor.is_some(),
            Route::Notifications => self.snapshot.notifications_next_cursor.is_some(),
            Route::Channel(_) | Route::Dms | Route::Search => false,
        }
    }

    fn detail_at_top(&self) -> bool {
        self.ui.detail_scroll.offset().y == 0
    }

    fn detail_at_bottom(&self) -> bool {
        let mut metrics = self.ui.detail_scroll_metrics;
        metrics.offset_y = self.ui.detail_scroll.offset().y;
        metrics.at_bottom()
    }

    fn queue_older_history_if_available(&mut self) {
        if self.can_load_older_history()
            && !self
                .actions
                .iter()
                .any(|action| matches!(action, Action::LoadOlder))
        {
            self.actions.push(Action::LoadOlder);
        }
    }

    fn queue_result_list_page_if_available(&mut self) {
        if self.can_load_more_result_list()
            && let Some(request) = self.current_load_more_request()
        {
            self.queue_load_more_request(request);
        }
    }

    pub(crate) fn reset_history_limit(&mut self) {
        self.history_limit = DEFAULT_HISTORY_LIMIT;
    }

    pub(crate) fn reset_detail_scroll(&mut self) {
        self.ui.detail_scroll.scroll_to_top();
        self.ui.detail_scroll_metrics = Default::default();
        self.ui.detail_selection_scroll_pending = false;
    }

    pub(crate) fn reset_detail_scroll_to_bottom(&mut self) {
        self.reset_detail_scroll();
        self.scroll_detail_to_bottom();
    }

    pub(crate) fn scroll_detail_to_bottom(&mut self) {
        self.ui
            .detail_scroll
            .set_offset(Position { x: 0, y: u16::MAX });
    }

    pub(crate) fn workspace_rows(&self) -> Vec<WorkspaceRow> {
        let mut rows = Vec::new();
        rows.push(WorkspaceRow::Notifications);
        rows.push(WorkspaceRow::Saved);
        for channel in &self.snapshot.channels {
            rows.push(WorkspaceRow::Channel(channel.id.clone()));
            let selected_channel = self.snapshot.selected_channel_id.as_deref()
                == Some(channel.id.as_str())
                && matches!(self.ui.route, Route::Channel(_));
            if selected_channel && !self.ui.threads_collapsed {
                rows.extend(
                    self.snapshot
                        .threads
                        .iter()
                        .map(|thread| WorkspaceRow::Thread(thread.id.clone())),
                );
            }
        }
        if self.snapshot.dm_sidebar.is_empty() {
            rows.extend(
                self.snapshot
                    .conversations
                    .iter()
                    .map(|dm| WorkspaceRow::Dm {
                        conversation_id: Some(dm.id.clone()),
                        username: dm.peer_username.clone(),
                    }),
            );
        } else {
            rows.extend(self.snapshot.dm_sidebar.iter().map(|dm| WorkspaceRow::Dm {
                conversation_id: dm.conversation_id.clone(),
                username: dm.peer_username.clone(),
            }));
        }
        let label_tags = self.workspace_label_tags();
        rows.extend(label_tags.iter().cloned().map(WorkspaceRow::Label));
        if !self.ui.labels_expanded && self.all_workspace_label_tags().len() > label_tags.len() {
            rows.push(WorkspaceRow::LabelsMore);
        }
        rows
    }

    pub(crate) fn workspace_label_tags(&self) -> Vec<String> {
        let mut tags = self.all_workspace_label_tags();
        if !self.ui.labels_expanded && tags.len() > 5 {
            tags.truncate(5);
        }
        tags
    }

    pub(crate) fn all_workspace_label_tags(&self) -> Vec<String> {
        let mut tags: Vec<String> = self
            .snapshot
            .hot_labels
            .iter()
            .map(|tag| tag.tag.clone())
            .collect();
        if let Route::Label(tag) = &self.ui.route
            && !tags.iter().any(|existing| existing == tag)
        {
            tags.push(tag.clone());
        }
        tags
    }

    pub(crate) fn current_workspace_row(&self) -> Option<WorkspaceRow> {
        match self.ui.active_pane {
            ActivePane::List if !self.ui.threads_collapsed => self
                .snapshot
                .selected_thread_id
                .as_ref()
                .map(|id| WorkspaceRow::Thread(id.clone()))
                .or_else(|| {
                    self.snapshot
                        .selected_channel_id
                        .as_ref()
                        .map(|id| WorkspaceRow::Channel(id.clone()))
                }),
            _ if matches!(self.ui.route, Route::Dms) => self
                .snapshot
                .selected_conversation_id
                .as_ref()
                .and_then(|id| {
                    self.snapshot
                        .dm_sidebar
                        .iter()
                        .find(|dm| dm.conversation_id.as_deref() == Some(id.as_str()))
                        .map(|dm| WorkspaceRow::Dm {
                            conversation_id: Some(id.clone()),
                            username: dm.peer_username.clone(),
                        })
                        .or_else(|| {
                            self.snapshot
                                .conversations
                                .iter()
                                .find(|dm| dm.id == *id)
                                .map(|dm| WorkspaceRow::Dm {
                                    conversation_id: Some(id.clone()),
                                    username: dm.peer_username.clone(),
                                })
                        })
                }),
            _ if matches!(self.ui.route, Route::Saved) => Some(WorkspaceRow::Saved),
            _ if matches!(self.ui.route, Route::Notifications) => Some(WorkspaceRow::Notifications),
            _ if matches!(self.ui.route, Route::Label(_)) => match &self.ui.route {
                Route::Label(tag) => Some(WorkspaceRow::Label(tag.clone())),
                _ => None,
            },
            _ => self
                .snapshot
                .selected_channel_id
                .as_ref()
                .map(|id| WorkspaceRow::Channel(id.clone())),
        }
    }

    pub(crate) fn move_workspace(&mut self, delta: isize) {
        let rows = self.workspace_rows();
        if rows.is_empty() {
            return;
        }
        let current = self
            .current_workspace_row()
            .and_then(|row| rows.iter().position(|candidate| candidate == &row))
            .unwrap_or(0);
        let next = clamp_index(current, delta, rows.len());
        self.apply_workspace_row(rows[next].clone());
    }

    pub(crate) fn apply_workspace_row(&mut self, row: WorkspaceRow) {
        match row {
            WorkspaceRow::Channel(channel_id) => {
                let changed = self.snapshot.selected_channel_id.as_deref()
                    != Some(channel_id.as_str())
                    || !matches!(self.ui.route, Route::Channel(_));
                self.clear_conversation_source_selection();
                self.snapshot.selected_channel_id = Some(channel_id.clone());
                self.ui.route = Route::Channel(channel_id);
                self.ui.active_pane = ActivePane::Rail;
                if changed {
                    self.snapshot.selected_thread_id = None;
                    self.snapshot.threads.clear();
                    self.snapshot.comments.clear();
                    self.reset_detail_scroll_to_bottom();
                    self.ui.threads_collapsed = false;
                    self.refresh_requested = true;
                }
            }
            WorkspaceRow::Thread(thread_id) => {
                let changed =
                    self.snapshot.selected_thread_id.as_deref() != Some(thread_id.as_str());
                self.clear_conversation_source_selection();
                self.snapshot.selected_thread_id = Some(thread_id);
                self.snapshot.comments.clear();
                if changed {
                    self.reset_detail_scroll_to_bottom();
                }
                self.ui.active_pane = ActivePane::List;
                self.ui.threads_collapsed = false;
                if let Some(channel_id) = self.snapshot.selected_channel_id.clone() {
                    self.ui.route = Route::Channel(channel_id);
                }
                self.refresh_requested = true;
            }
            WorkspaceRow::Saved => {
                self.ui.route = Route::Saved;
                self.ui.active_pane = ActivePane::Detail;
                self.clear_active_source_selection();
                self.reset_detail_scroll();
                self.actions.push(Action::ListSaved);
            }
            WorkspaceRow::Label(tag) => {
                self.ui.route = Route::Label(tag.clone());
                self.ui.active_pane = ActivePane::Detail;
                self.snapshot.selected_thread_id = None;
                self.snapshot.selected_conversation_id = None;
                self.reset_detail_scroll();
                self.actions.push(Action::OpenLabel { tag });
            }
            WorkspaceRow::LabelsMore => {
                self.ui.labels_expanded = true;
                self.ui.active_pane = ActivePane::Rail;
            }
            WorkspaceRow::Notifications => {
                self.ui.route = Route::Notifications;
                self.ui.active_pane = ActivePane::Detail;
                self.clear_active_source_selection();
                self.reset_detail_scroll();
                self.actions.push(Action::ListNotifications);
            }
            WorkspaceRow::Dm {
                conversation_id,
                username,
            } => {
                if let Some(conversation_id) = conversation_id {
                    let changed = self.snapshot.selected_conversation_id.as_deref()
                        != Some(conversation_id.as_str());
                    self.clear_channel_source_selection();
                    self.snapshot.selected_conversation_id = Some(conversation_id);
                    self.ui.route = Route::Dms;
                    self.ui.active_pane = ActivePane::Rail;
                    if changed {
                        self.snapshot.conversation_messages.clear();
                        self.reset_detail_scroll();
                        self.refresh_requested = true;
                    }
                } else {
                    self.actions.push(Action::OpenDm { target: username });
                }
            }
        }
    }

    pub(crate) fn activate_selection(&mut self) {
        if self.ui.route == Route::Search {
            self.activate_search_result();
            return;
        }
        if matches!(self.ui.route, Route::Label(_)) && self.ui.active_pane == ActivePane::Detail {
            self.activate_label_result();
            return;
        }
        if self.ui.route == Route::Saved && self.ui.active_pane == ActivePane::Detail {
            self.activate_saved_result();
            return;
        }
        if self.ui.route == Route::Notifications && self.ui.active_pane == ActivePane::Detail {
            self.activate_notification_result();
            return;
        }
        match self.ui.active_pane {
            ActivePane::Detail => self.enter_compose(""),
            ActivePane::Rail => {
                if matches!(
                    self.ui.route,
                    Route::Dms | Route::Label(_) | Route::Saved | Route::Notifications
                ) {
                    self.ui.active_pane = ActivePane::Detail;
                } else if self.ui.threads_collapsed {
                    self.ui.threads_collapsed = false;
                } else if let Some(thread_id) =
                    self.snapshot.selected_thread_id.clone().or_else(|| {
                        self.snapshot
                            .threads
                            .first()
                            .map(|thread| thread.id.clone())
                    })
                {
                    let changed =
                        self.snapshot.selected_thread_id.as_deref() != Some(thread_id.as_str());
                    self.snapshot.selected_thread_id = Some(thread_id);
                    if changed {
                        self.reset_detail_scroll_to_bottom();
                    }
                    self.ui.active_pane = ActivePane::List;
                } else {
                    self.toggle_threads();
                }
            }
            ActivePane::List => {
                self.ui.active_pane = ActivePane::Detail;
                self.scroll_detail_to_bottom();
                self.actions.push(Action::MarkThreadRead);
            }
        }
    }

    pub(crate) fn activate_search_result(&mut self) {
        let Some(result) = self
            .snapshot
            .search_results
            .get(self.ui.search_selected)
            .cloned()
        else {
            return;
        };
        if let Some(conversation_id) = result.conversation_id {
            self.select_conversation(conversation_id);
        } else if let (Some(channel_id), Some(thread_id)) = (result.channel_id, result.thread_id) {
            self.select_thread(channel_id, thread_id);
        }
    }

    pub(crate) fn activate_label_result(&mut self) {
        let Some(item) = self
            .snapshot
            .label_items
            .get(self.ui.label_selected)
            .cloned()
        else {
            return;
        };
        let focus = match item.kind {
            LabelFeedKind::Thread => SourceFocus::ThreadRoot,
            LabelFeedKind::Comment => item
                .source_obj_index
                .map(SourceFocus::Comment)
                .unwrap_or(SourceFocus::ThreadRoot),
            LabelFeedKind::Dm => item
                .source_obj_index
                .map(SourceFocus::Dm)
                .unwrap_or(SourceFocus::Dm(0)),
        };
        if let (Some(channel_id), Some(thread_id)) = (item.channel_id, item.thread_id) {
            self.select_thread_with_focus(channel_id, thread_id, focus);
        } else if let Some(conversation_id) = item.conversation_id {
            self.select_conversation_with_focus(conversation_id, focus);
        }
    }

    pub(crate) fn activate_saved_result(&mut self) {
        let Some(item) = self
            .snapshot
            .saved_messages
            .get(self.ui.saved_selected)
            .cloned()
        else {
            return;
        };
        let focus = match &item.kind {
            SavedMessageKind::Comment => SourceFocus::Comment(item.source_obj_index),
            SavedMessageKind::Dm => SourceFocus::Dm(item.source_obj_index),
        };
        match item.kind {
            SavedMessageKind::Dm => {
                if let Some(conversation_id) = item.conversation_id {
                    self.select_conversation_with_focus(conversation_id, focus);
                } else if let (Some(channel_id), Some(thread_id)) =
                    (item.channel_id, item.thread_id)
                {
                    self.select_thread_with_focus(channel_id, thread_id, focus);
                }
            }
            SavedMessageKind::Comment => {
                if let (Some(channel_id), Some(thread_id)) = (item.channel_id, item.thread_id) {
                    self.select_thread_with_focus(channel_id, thread_id, focus);
                } else if let Some(conversation_id) = item.conversation_id {
                    self.select_conversation_with_focus(conversation_id, focus);
                }
            }
        }
    }

    pub(crate) fn activate_notification_result(&mut self) {
        let Some(index) = self
            .visible_notification_indices()
            .get(self.ui.notifications_selected)
            .copied()
        else {
            return;
        };
        let Some(notification) = self.snapshot.notifications.get(index).cloned() else {
            return;
        };
        if notification.conversation_id.is_none() && notification.channel_id.is_none() {
            return;
        }
        let focus = match notification.source_kind.as_deref() {
            Some("thread") => Some(SourceFocus::ThreadRoot),
            Some("comment") => notification.source_obj_index.map(SourceFocus::Comment),
            Some("dm") => notification.source_obj_index.map(SourceFocus::Dm),
            _ => None,
        };
        let is_dm_source = notification.source_kind.as_deref() == Some("dm");
        let conversation_id = notification.conversation_id.clone();
        if is_dm_source && let Some(conversation_id) = conversation_id.clone() {
            if let Some(focus) = focus {
                self.select_conversation_with_focus(conversation_id, focus);
            } else {
                self.select_conversation(conversation_id);
            }
        } else if let (Some(channel_id), Some(thread_id)) =
            (notification.channel_id, notification.thread_id)
        {
            if let Some(focus) = focus {
                self.select_thread_with_focus(channel_id, thread_id, focus);
            } else {
                self.select_thread(channel_id, thread_id);
            }
        } else if let Some(conversation_id) = notification.conversation_id {
            if let Some(focus) = focus {
                self.select_conversation_with_focus(conversation_id, focus);
            } else {
                self.select_conversation(conversation_id);
            }
        }
    }

    pub(crate) fn visible_notification_indices(&self) -> Vec<usize> {
        visible_notification_indices(&self.snapshot.notifications, self.ui.notification_filter)
    }

    pub(crate) fn set_notification_filter(&mut self, filter: NotificationFilter) {
        self.ui.notification_filter = filter;
        self.ui.notifications_selected = 0;
        self.reset_detail_scroll();
    }

    pub(crate) fn cycle_notification_filter(&mut self) {
        if self.ui.route == Route::Notifications {
            self.set_notification_filter(self.ui.notification_filter.next());
        }
    }

    pub(crate) fn navigate_left(&mut self) {
        match self.ui.active_pane {
            ActivePane::Detail => {
                self.ui.active_pane = if self.snapshot.selected_thread_id.is_some()
                    && matches!(self.ui.route, Route::Channel(_))
                {
                    ActivePane::List
                } else {
                    ActivePane::Rail
                };
            }
            ActivePane::List => self.ui.active_pane = ActivePane::Rail,
            ActivePane::Rail
                if matches!(self.ui.route, Route::Channel(_)) && !self.ui.threads_collapsed =>
            {
                self.ui.threads_collapsed = true;
            }
            _ => {}
        }
    }

    pub(crate) fn navigate_right(&mut self) {
        match self.ui.active_pane {
            ActivePane::Detail => {}
            ActivePane::Rail
                if matches!(
                    self.ui.route,
                    Route::Label(_) | Route::Saved | Route::Notifications
                ) =>
            {
                self.ui.active_pane = ActivePane::Detail;
            }
            ActivePane::List => self.ui.active_pane = ActivePane::Detail,
            ActivePane::Rail if matches!(self.ui.route, Route::Dms) => {
                self.ui.active_pane = ActivePane::Detail;
            }
            ActivePane::Rail if self.ui.threads_collapsed => {
                self.ui.threads_collapsed = false;
            }
            ActivePane::Rail => {
                if let Some(thread_id) = self.snapshot.selected_thread_id.clone().or_else(|| {
                    self.snapshot
                        .threads
                        .first()
                        .map(|thread| thread.id.clone())
                }) {
                    let changed =
                        self.snapshot.selected_thread_id.as_deref() != Some(thread_id.as_str());
                    self.snapshot.selected_thread_id = Some(thread_id);
                    if changed {
                        self.reset_detail_scroll_to_bottom();
                    }
                    self.ui.active_pane = ActivePane::List;
                }
            }
        }
    }

    pub(crate) fn toggle_workspace_detail(&mut self) {
        if self.ui.active_pane == ActivePane::Detail {
            self.ui.active_pane = if self.snapshot.selected_thread_id.is_some()
                && matches!(self.ui.route, Route::Channel(_))
            {
                ActivePane::List
            } else {
                ActivePane::Rail
            };
            return;
        }

        if matches!(self.ui.route, Route::Dms) {
            self.ui.active_pane = ActivePane::Detail;
            self.actions.push(Action::MarkDmRead);
            return;
        }

        if matches!(
            self.ui.route,
            Route::Label(_) | Route::Saved | Route::Notifications
        ) {
            self.ui.active_pane = ActivePane::Detail;
            return;
        }

        if self.snapshot.selected_thread_id.is_none()
            && let Some(thread) = self.snapshot.threads.first()
        {
            self.snapshot.selected_thread_id = Some(thread.id.clone());
            self.reset_detail_scroll_to_bottom();
        }
        if self.snapshot.selected_thread_id.is_some() {
            self.ui.active_pane = ActivePane::Detail;
            self.actions.push(Action::MarkThreadRead);
        }
    }

    pub(crate) fn toggle_threads(&mut self) {
        if matches!(self.ui.route, Route::Channel(_)) {
            self.ui.threads_collapsed = !self.ui.threads_collapsed;
            if self.ui.threads_collapsed {
                self.ui.active_pane = ActivePane::Rail;
            }
        }
    }
}

pub(crate) fn visible_notification_indices(
    notifications: &[NotificationSummary],
    filter: NotificationFilter,
) -> Vec<usize> {
    notifications
        .iter()
        .enumerate()
        .filter_map(|(idx, notification)| {
            let visible = match filter {
                NotificationFilter::All => true,
                NotificationFilter::Unread => notification.read_at.is_none(),
                NotificationFilter::Read => notification.read_at.is_some(),
            };
            visible.then_some(idx)
        })
        .collect()
}
