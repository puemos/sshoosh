use super::*;
impl App {
    pub(crate) fn move_selection(&mut self, delta: isize) {
        if self.ui.route == Route::Search {
            self.move_search(delta);
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
            for _ in 0..delta.unsigned_abs() {
                self.ui.detail_scroll.scroll_up();
            }
        } else {
            for _ in 0..delta as usize {
                self.ui.detail_scroll.scroll_down();
            }
        }
    }

    pub(crate) fn move_search(&mut self, delta: isize) {
        let len = self.snapshot.search_results.len();
        if len == 0 {
            return;
        }
        self.ui.search_selected = clamp_index(self.ui.search_selected, delta, len);
    }

    pub(crate) fn reset_history_limit(&mut self) {
        self.history_limit = DEFAULT_HISTORY_LIMIT;
    }

    pub(crate) fn reset_detail_scroll(&mut self) {
        self.ui.detail_scroll.scroll_to_top();
    }

    pub(crate) fn scroll_detail_to_bottom(&mut self) {
        self.ui
            .detail_scroll
            .set_offset(Position { x: 0, y: u16::MAX });
    }

    pub(crate) fn workspace_rows(&self) -> Vec<WorkspaceRow> {
        let mut rows = Vec::new();
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
        rows.extend(
            self.snapshot
                .conversations
                .iter()
                .map(|dm| WorkspaceRow::Dm(dm.id.clone())),
        );
        rows
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
                .map(|id| WorkspaceRow::Dm(id.clone())),
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
                self.snapshot.selected_channel_id = Some(channel_id.clone());
                self.snapshot.selected_conversation_id = None;
                self.ui.route = Route::Channel(channel_id);
                self.ui.active_pane = ActivePane::Rail;
                if changed {
                    self.snapshot.selected_thread_id = None;
                    self.snapshot.threads.clear();
                    self.snapshot.comments.clear();
                    self.reset_detail_scroll();
                    self.ui.threads_collapsed = false;
                    self.refresh_requested = true;
                }
            }
            WorkspaceRow::Thread(thread_id) => {
                let changed =
                    self.snapshot.selected_thread_id.as_deref() != Some(thread_id.as_str());
                self.snapshot.selected_thread_id = Some(thread_id);
                self.snapshot.selected_conversation_id = None;
                self.snapshot.comments.clear();
                if changed {
                    self.reset_detail_scroll();
                }
                self.ui.active_pane = ActivePane::List;
                self.ui.threads_collapsed = false;
                if let Some(channel_id) = self.snapshot.selected_channel_id.clone() {
                    self.ui.route = Route::Channel(channel_id);
                }
                self.refresh_requested = true;
            }
            WorkspaceRow::Dm(conversation_id) => {
                let changed = self.snapshot.selected_conversation_id.as_deref()
                    != Some(conversation_id.as_str());
                self.snapshot.selected_conversation_id = Some(conversation_id);
                self.ui.route = Route::Dms;
                self.ui.active_pane = ActivePane::Rail;
                if changed {
                    self.snapshot.conversation_messages.clear();
                    self.reset_detail_scroll();
                    self.refresh_requested = true;
                }
            }
        }
    }

    pub(crate) fn activate_selection(&mut self) {
        if self.ui.route == Route::Search {
            self.activate_search_result();
            return;
        }
        match self.ui.active_pane {
            ActivePane::Detail => self.enter_compose(""),
            ActivePane::Rail => {
                if matches!(self.ui.route, Route::Dms) {
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
                        self.reset_detail_scroll();
                    }
                    self.ui.active_pane = ActivePane::List;
                } else {
                    self.toggle_threads();
                }
            }
            ActivePane::List => {
                self.ui.active_pane = ActivePane::Detail;
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
        if let (Some(channel_id), Some(thread_id)) = (result.channel_id, result.thread_id) {
            self.select_thread(channel_id, thread_id);
        } else if let Some(conversation_id) = result.conversation_id {
            self.select_conversation(conversation_id);
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
                        self.reset_detail_scroll();
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

        if self.snapshot.selected_thread_id.is_none()
            && let Some(thread) = self.snapshot.threads.first()
        {
            self.snapshot.selected_thread_id = Some(thread.id.clone());
            self.reset_detail_scroll();
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
