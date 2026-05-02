use super::*;
impl App {
    pub(crate) fn enter_compose(&mut self, initial: &str) {
        self.ui.mode = UiMode::Compose;
        self.ui.composer.start(initial);
        self.update_completions();
    }

    pub(crate) fn open_compose_prompt(&mut self, _title: &str, prefix: &str, placeholder: &str) {
        self.ui.mode = UiMode::Compose;
        self.ui.composer.start_prompt(prefix, placeholder);
        self.update_completions();
    }

    pub(crate) fn open_palette(&mut self) {
        self.ui.mode = UiMode::Palette;
        self.ui.palette = PaletteState::default();
        self.rebuild_palette();
    }

    pub(crate) fn open_help(&mut self) {
        self.ui.mode = UiMode::Help;
        self.ui.help_scroll.scroll_to_top();
    }

    pub(crate) fn rebuild_palette(&mut self) {
        self.ui.palette.items = self.commands.palette_items(&self.snapshot);
        let query = self.ui.palette.query.clone();
        self.ui.palette.apply_filter(&query);
    }

    pub(crate) fn run_palette_selection(&mut self) {
        let Some(item) = self.ui.palette.selected_item().cloned() else {
            return;
        };
        self.run_command_executor(item.executor);
    }

    pub(crate) fn run_command_executor(&mut self, executor: CommandExecutor) {
        self.ui.mode = UiMode::Normal;
        match executor {
            CommandExecutor::Action(action) => self.actions.push(action),
            CommandExecutor::Prompt {
                title,
                prefix,
                placeholder,
            } => self.open_compose_prompt(&title, &prefix, &placeholder),
            CommandExecutor::SwitchChannel(id) => {
                self.snapshot.selected_channel_id = Some(id.clone());
                self.snapshot.selected_conversation_id = None;
                self.snapshot.selected_thread_id = None;
                self.snapshot.threads.clear();
                self.snapshot.comments.clear();
                self.reset_detail_scroll();
                self.ui.active_pane = ActivePane::List;
                self.ui.route = Route::Channel(id);
                self.ui.threads_collapsed = false;
                self.refresh_requested = true;
            }
            CommandExecutor::SwitchDm(id) => {
                self.snapshot.selected_conversation_id = Some(id);
                self.snapshot.conversation_messages.clear();
                self.reset_detail_scroll();
                self.ui.route = Route::Dms;
                self.ui.active_pane = ActivePane::Detail;
                self.refresh_requested = true;
            }
            CommandExecutor::SwitchThread(id) => {
                self.snapshot.selected_thread_id = Some(id);
                self.snapshot.comments.clear();
                self.reset_detail_scroll();
                self.ui.active_pane = ActivePane::Detail;
                self.ui.threads_collapsed = false;
                self.refresh_requested = true;
            }
            CommandExecutor::Mode(UiMode::Help) => self.open_help(),
            CommandExecutor::Mode(mode) => self.ui.mode = mode,
            CommandExecutor::Quit => self.ui.mode = UiMode::ConfirmQuit,
        }
    }

    pub(crate) fn update_completions(&mut self) {
        self.ui.composer.autocomplete = self.commands.autocomplete(
            &self.ui.composer.buffer,
            self.ui.composer.cursor,
            &self.snapshot,
        );
    }

    pub(crate) fn accept_autocomplete_enter(&mut self) -> bool {
        self.accept_autocomplete_selection(false)
    }

    pub(crate) fn accept_autocomplete_tab(&mut self) -> bool {
        self.accept_autocomplete_selection(true)
    }

    pub(crate) fn accept_autocomplete_selection(&mut self, refresh_after_insert: bool) -> bool {
        if !self.ui.composer.autocomplete.open {
            return false;
        }
        let Some(item) = self
            .ui
            .composer
            .autocomplete
            .items
            .get(self.ui.composer.autocomplete.selected)
            .cloned()
        else {
            self.ui.composer.autocomplete.open = false;
            return false;
        };
        if !refresh_after_insert && !item.accept_on_enter {
            let active_token = self
                .ui
                .composer
                .buffer
                .get(item.replacement_range.clone())
                .unwrap_or_default();
            if active_token.starts_with(':') {
                return false;
            }
        }
        if refresh_after_insert && !item.accept_on_tab {
            return false;
        }
        if let Some(executor) = item.executor {
            self.ui.composer.reset_input();
            self.run_command_executor(executor);
            return true;
        }
        self.ui
            .composer
            .replace_range(item.replacement_range, &item.replacement);
        self.ui.composer.autocomplete.open = false;
        if refresh_after_insert {
            self.update_completions();
        }
        true
    }

    pub(crate) fn submit_onboarding(&mut self) {
        let username = self.ui.composer.buffer.trim().to_string();
        self.ui.composer.reset_input();
        if username.is_empty() {
            self.set_banner_err("Username is required");
            return;
        }
        self.actions.push(Action::CompleteOnboarding { username });
    }

    pub(crate) fn submit_composer(&mut self) {
        let body = self.ui.composer.buffer.trim().to_string();
        if body.is_empty() {
            self.ui.mode = UiMode::Normal;
            self.ui.composer.reset_input();
            return;
        }
        self.ui.composer.push_history(body.clone());
        self.ui.composer.reset_input();
        self.ui.mode = UiMode::Normal;

        if body.starts_with('/') {
            self.dispatch_command_line(body);
        } else if matches!(self.ui.route, Route::Dms)
            || self.ui.active_pane == ActivePane::Detail
                && self.snapshot.selected_conversation_id.is_some()
        {
            self.actions.push(Action::SendDm { body });
        } else {
            self.actions.push(Action::AddComment { body });
        }
    }

    pub(crate) fn dispatch_command_line(&mut self, line: String) {
        let command = line
            .trim()
            .trim_start_matches('/')
            .split_whitespace()
            .next()
            .unwrap_or_default();
        if matches!(command, "help" | "?") {
            self.open_help();
            return;
        }
        if matches!(command, "quit" | "q") {
            self.ui.mode = UiMode::ConfirmQuit;
            return;
        }
        match self.commands.parse_action(&line) {
            Ok(action) => {
                if let Some(action) = action {
                    self.actions.push(action);
                }
            }
            Err(message) => self.set_banner_err(message),
        }
    }
}
