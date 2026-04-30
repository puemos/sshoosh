use super::*;
impl App {
    pub(crate) fn enter_compose(&mut self, initial: &str) {
        self.ui.mode = UiMode::Compose;
        self.ui.composer = ComposerState::from(initial);
        self.update_completions();
    }

    pub(crate) fn open_prompt(&mut self, title: &str, prefix: &str, placeholder: &str) {
        self.ui.mode = UiMode::Prompt;
        self.ui.prompt = PromptState {
            title: title.to_string(),
            prefix: prefix.to_string(),
            placeholder: placeholder.to_string(),
            input: String::new(),
        };
    }

    pub(crate) fn open_palette(&mut self) {
        self.ui.mode = UiMode::Palette;
        self.ui.palette = PaletteState::default();
        self.rebuild_palette();
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
        self.ui.mode = UiMode::Normal;
        match item.executor {
            CommandExecutor::Action(action) => self.actions.push(action),
            CommandExecutor::Prompt {
                title,
                prefix,
                placeholder,
            } => self.open_prompt(&title, &prefix, &placeholder),
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

    pub(crate) fn accept_autocomplete_if_incomplete(&mut self) -> bool {
        let replacement = self.ui.composer.autocomplete.selected_replacement();
        if let Some((range, value)) = replacement {
            if self.ui.composer.buffer.get(range.clone()) == Some(value.as_str()) {
                return false;
            }
            self.ui.composer.replace_range(range, &value);
            self.update_completions();
            return true;
        }
        false
    }

    pub(crate) fn accept_autocomplete_tab(&mut self) -> bool {
        let replacement = self.ui.composer.autocomplete.selected_tab_replacement();
        if let Some((range, value)) = replacement {
            self.ui.composer.replace_range(range, &value);
            self.update_completions();
            return true;
        }
        false
    }

    pub(crate) fn submit_onboarding(&mut self) {
        let body = self.ui.composer.buffer.trim().to_string();
        self.ui.composer = ComposerState::default();
        if body.is_empty() {
            return;
        }
        let mut parts = body.split_whitespace();
        let code = match parts.next() {
            Some("/join") => parts.next().unwrap_or_default().to_string(),
            Some(value) => value.to_string(),
            None => String::new(),
        };
        let suggested_username = self
            .account
            .pending_username
            .as_deref()
            .unwrap_or(&self.account.username);
        let username = parts.next().unwrap_or(suggested_username).to_string();
        self.actions.push(Action::AcceptInvite { code, username });
    }

    pub(crate) fn submit_composer(&mut self) {
        let body = self.ui.composer.buffer.trim().to_string();
        if body.is_empty() {
            self.ui.mode = UiMode::Normal;
            return;
        }
        self.ui.composer.push_history(body.clone());
        self.ui.composer = ComposerState::default();
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
            self.ui.mode = UiMode::Help;
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
