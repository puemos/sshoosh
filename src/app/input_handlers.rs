use super::*;
impl App {
    pub fn handle_input(&mut self, bytes: &[u8]) {
        let keys = self.decoder.push(bytes);
        for key in keys {
            self.handle_key(key);
            if !self.running {
                break;
            }
        }
    }

    pub(crate) fn handle_key(&mut self, key: Key) {
        if let Key::Mouse(mouse) = key {
            self.handle_mouse(mouse);
            return;
        }

        self.ui.selection.clear();

        if matches!(key, Key::Ctrl('c') | Key::Ctrl('d')) {
            self.running = false;
            return;
        }

        if self
            .ui
            .banner
            .as_ref()
            .is_some_and(|banner| banner.modal_active())
        {
            match key {
                Key::Esc | Key::Enter | Key::ShiftEnter => {
                    self.ui.banner = None;
                }
                Key::Char('c') | Key::Char('C') => {
                    if let Some(code) = self.active_invite_code().map(str::to_string) {
                        self.pending_clipboard_copy = Some(code);
                        self.ui.banner = Some(Banner::ok("Invite code copied"));
                    }
                }
                _ => {}
            }
            return;
        }

        if !self.account.activated {
            self.handle_onboarding_key(key);
            return;
        }

        match self.ui.mode {
            UiMode::Normal => self.handle_normal_key(key),
            UiMode::Compose => self.handle_compose_key(key),
            UiMode::Palette => self.handle_palette_key(key),
            UiMode::Prompt => self.handle_prompt_key(key),
            UiMode::Help => self.handle_help_key(key),
            UiMode::ConfirmQuit => self.handle_confirm_quit_key(key),
        }
    }

    pub(crate) fn handle_mouse(&mut self, mouse: MouseEvent) {
        if self
            .ui
            .banner
            .as_ref()
            .is_some_and(|banner| banner.modal_active())
        {
            self.ui.selection.clear();
            return;
        }

        match mouse.kind {
            MouseEventKind::ScrollUp => {
                self.ui.selection.clear();
                if let Some(region) = self.ui.hit_map.hit(mouse.column, mouse.row) {
                    self.handle_mouse_scroll(&region.target, -3);
                }
            }
            MouseEventKind::ScrollDown => {
                self.ui.selection.clear();
                if let Some(region) = self.ui.hit_map.hit(mouse.column, mouse.row) {
                    self.handle_mouse_scroll(&region.target, 3);
                }
            }
            MouseEventKind::ScrollLeft | MouseEventKind::ScrollRight => {}
            MouseEventKind::Down(MouseButton::Left) => self.start_mouse_click_or_selection(mouse),
            MouseEventKind::Drag(MouseButton::Left) => self.update_mouse_selection(mouse),
            MouseEventKind::Up(MouseButton::Left) => self.finish_mouse_click_or_selection(mouse),
            MouseEventKind::Down(_) => self.ui.selection.clear(),
            MouseEventKind::Moved => self.update_pointer_shape(mouse),
            MouseEventKind::Up(_) | MouseEventKind::Drag(_) => {}
        }
    }

    pub(crate) fn update_pointer_shape(&mut self, mouse: MouseEvent) {
        self.desired_pointer_shape = match self.ui.hit_map.hit(mouse.column, mouse.row) {
            Some(HitRegion {
                target: HitTarget::MessageLink(_),
                ..
            }) => PointerShape::Pointer,
            _ => PointerShape::Default,
        };
    }

    pub(crate) fn handle_mouse_scroll(&mut self, target: &HitTarget, delta: isize) {
        match target {
            HitTarget::WorkspaceScroll
            | HitTarget::WorkspaceChannel(_)
            | HitTarget::WorkspaceThread(_)
            | HitTarget::WorkspaceDm(_) => self.move_workspace(delta),
            HitTarget::DetailScroll | HitTarget::MessageLink(_) => self.move_detail(delta),
            HitTarget::AutocompleteScroll | HitTarget::AutocompleteRow(_) => {
                let steps = delta.unsigned_abs().max(1);
                for _ in 0..steps {
                    if delta < 0 {
                        self.ui.composer.autocomplete.previous();
                    } else {
                        self.ui.composer.autocomplete.next();
                    }
                }
            }
            HitTarget::PaletteResults | HitTarget::PaletteRow(_) => {
                let steps = delta.unsigned_abs().max(1);
                for _ in 0..steps {
                    if delta < 0 {
                        self.ui.palette.previous();
                    } else {
                        self.ui.palette.next();
                    }
                }
            }
            _ => {}
        }
    }

    pub(crate) fn start_mouse_click_or_selection(&mut self, mouse: MouseEvent) {
        self.ui.selection.clear();
        self.ui.selection.pending = Some(SelectionAnchor {
            at: mouse_position(mouse),
            region: self.ui.hit_map.hit(mouse.column, mouse.row),
            modifiers: mouse.modifiers,
            moved: false,
        });
    }

    pub(crate) fn update_mouse_selection(&mut self, mouse: MouseEvent) {
        let Some(anchor) = self.ui.selection.pending.as_mut() else {
            return;
        };
        let end = mouse_position(mouse);
        if end == anchor.at && !anchor.moved {
            return;
        }
        anchor.moved = true;
        self.ui.selection.range = Some(SelectionRange {
            start: anchor.at,
            end,
        });
    }

    pub(crate) fn finish_mouse_click_or_selection(&mut self, mouse: MouseEvent) {
        let Some(mut anchor) = self.ui.selection.pending.take() else {
            return;
        };
        let end = mouse_position(mouse);
        if end != anchor.at {
            anchor.moved = true;
            self.ui.selection.range = Some(SelectionRange {
                start: anchor.at,
                end,
            });
        }
        if anchor.moved || self.ui.selection.range.is_some() {
            self.ui.selection.copy_requested = true;
            return;
        }
        self.ui.selection.clear();
        if let Some(region) = anchor
            .region
            .or_else(|| self.ui.hit_map.hit(mouse.column, mouse.row))
        {
            let mut mouse = mouse;
            mouse.modifiers.shift |= anchor.modifiers.shift;
            mouse.modifiers.alt |= anchor.modifiers.alt;
            mouse.modifiers.control |= anchor.modifiers.control;
            self.handle_mouse_click(region, mouse);
        }
    }

    pub(crate) fn handle_mouse_click(&mut self, region: HitRegion, mouse: MouseEvent) {
        match region.target {
            HitTarget::WorkspaceChannel(channel_id) => self.select_channel(channel_id),
            HitTarget::WorkspaceThread(thread_id) => {
                if let Some(channel_id) = self
                    .snapshot
                    .threads
                    .iter()
                    .find(|thread| thread.id == thread_id)
                    .map(|thread| thread.channel_id.clone())
                    .or_else(|| self.snapshot.selected_channel_id.clone())
                {
                    self.select_thread(channel_id, thread_id);
                }
            }
            HitTarget::WorkspaceDm(conversation_id) => self.select_conversation(conversation_id),
            HitTarget::WorkspaceScroll => self.ui.active_pane = ActivePane::Rail,
            HitTarget::DetailScroll => self.ui.active_pane = ActivePane::Detail,
            HitTarget::MessageLink(url) => {
                self.ui.active_pane = ActivePane::Detail;
                self.pending_link_open = Some(url);
            }
            HitTarget::ComposerInput { scroll_y } => {
                if self.account.activated && self.ui.mode != UiMode::Compose {
                    self.ui.mode = UiMode::Compose;
                }
                self.place_composer_cursor(region.rect, scroll_y, mouse);
            }
            HitTarget::AutocompleteRow(idx) => {
                if idx < self.ui.composer.autocomplete.items.len() {
                    self.ui.composer.autocomplete.selected = idx;
                    let _ = self.accept_autocomplete_tab();
                }
            }
            HitTarget::AutocompleteScroll => {}
            HitTarget::PaletteRow(row) => {
                if row < self.ui.palette.filtered.len() {
                    self.ui.palette.selected = row;
                    self.run_palette_selection();
                }
            }
            HitTarget::PaletteInput | HitTarget::PaletteResults => {}
            HitTarget::PaletteBackdrop | HitTarget::PromptBackdrop | HitTarget::HelpBackdrop => {
                self.ui.mode = UiMode::Normal;
            }
            HitTarget::PromptInput => {}
            HitTarget::BannerModal => {}
            HitTarget::ConfirmQuitYes => self.running = false,
            HitTarget::ConfirmQuitNo | HitTarget::ConfirmQuitBackdrop => {
                self.ui.mode = UiMode::Normal;
            }
            HitTarget::BottomBar(action) => self.run_bottom_bar_action(action),
        }
    }

    pub(crate) fn place_composer_cursor(&mut self, rect: Rect, scroll_y: u16, mouse: MouseEvent) {
        let local_col = mouse.column.saturating_sub(rect.x) as usize;
        let display_line = mouse.row.saturating_sub(rect.y).saturating_add(scroll_y) as usize;
        self.ui.composer.cursor = cursor_for_display_position(
            &self.ui.composer.buffer,
            rect.width.max(1) as usize,
            display_line,
            local_col,
        );
        self.update_completions();
    }

    pub(crate) fn run_bottom_bar_action(&mut self, action: BottomBarAction) {
        match action {
            BottomBarAction::ToggleDetail => self.toggle_workspace_detail(),
            BottomBarAction::OpenCommand => self.enter_compose("/"),
            BottomBarAction::OpenHelp => self.ui.mode = UiMode::Help,
            BottomBarAction::OpenQuit => self.ui.mode = UiMode::ConfirmQuit,
            BottomBarAction::SubmitComposer => self.submit_composer(),
            BottomBarAction::AcceptAutocomplete => {
                let _ = self.accept_autocomplete_tab();
            }
            BottomBarAction::CloseMode => {
                if self.ui.mode == UiMode::Compose {
                    self.ui.composer.reset_input();
                }
                self.ui.mode = UiMode::Normal;
            }
            BottomBarAction::RunPalette => self.run_palette_selection(),
            BottomBarAction::RunPrompt => {
                let value = self.ui.prompt.input.trim().to_string();
                let prefix = self.ui.prompt.prefix.clone();
                self.ui.mode = UiMode::Normal;
                self.dispatch_command_line(format!("{prefix}{value}"));
            }
            BottomBarAction::ConfirmQuit => self.running = false,
            BottomBarAction::CancelQuit => self.ui.mode = UiMode::Normal,
        }
    }

    pub(crate) fn handle_onboarding_key(&mut self, key: Key) {
        match key {
            Key::Enter | Key::ShiftEnter => self.submit_onboarding(),
            Key::Backspace => self.ui.composer.backspace(),
            Key::Delete => self.ui.composer.delete(),
            Key::Left | Key::Ctrl('b') => self.ui.composer.move_left(),
            Key::Right | Key::Ctrl('f') => self.ui.composer.move_right(),
            Key::Ctrl('a') | Key::Home => self.ui.composer.cursor = 0,
            Key::Ctrl('e') | Key::End => self.ui.composer.cursor = self.ui.composer.buffer.len(),
            Key::Ctrl('u') => self.ui.composer.clear_before_cursor(),
            Key::Ctrl('k') => self.ui.composer.clear_after_cursor(),
            Key::Paste(text) => self.ui.composer.insert_str(&text),
            Key::Char(ch) => self.ui.composer.insert(ch),
            _ => {}
        }
    }

    pub(crate) fn handle_normal_key(&mut self, key: Key) {
        match key {
            Key::Char('q') => self.ui.mode = UiMode::ConfirmQuit,
            Key::Char('?') | Key::CtrlSeq('x', 'h') => self.ui.mode = UiMode::Help,
            Key::Ctrl('p') | Key::CtrlSeq('x', 'p') => self.open_palette(),
            Key::Tab | Key::BackTab => self.toggle_workspace_detail(),
            Key::Left | Key::Char('h') => self.navigate_left(),
            Key::Right | Key::Char('l') => self.navigate_right(),
            Key::Down | Key::Char('j') => self.move_selection(1),
            Key::Up | Key::Char('k') => self.move_selection(-1),
            Key::PageDown if self.ui.active_pane == ActivePane::Detail => {
                self.ui.detail_scroll.scroll_page_down();
            }
            Key::PageUp if self.ui.active_pane == ActivePane::Detail => {
                self.ui.detail_scroll.scroll_page_up();
            }
            Key::PageDown => self.move_selection(8),
            Key::PageUp => self.move_selection(-8),
            Key::Home | Key::Char('g') => self.move_to_edge(false),
            Key::End | Key::Char('G') => self.move_to_edge(true),
            Key::Enter | Key::ShiftEnter => self.activate_selection(),
            Key::Char('i') | Key::Char('r') => self.enter_compose(""),
            Key::Char('/') => self.enter_compose("/"),
            Key::Char(' ') => self.toggle_threads(),
            Key::Char('t') => self.enter_compose("/thread new "),
            Key::Char('d') => self.enter_compose("/dm open "),
            Key::Char('c') => self.enter_compose("/channel new "),
            Key::Char('n') => self.actions.push(Action::NextUnread),
            Key::Char('N') => self.actions.push(Action::NextUnread),
            Key::Char('m') => self.actions.push(Action::MarkThreadRead),
            Key::Char('u') => self.actions.push(Action::MarkThreadUnread),
            Key::Char(ch) if !ch.is_control() => {
                self.enter_compose("");
                self.handle_compose_key(Key::Char(ch));
            }
            _ => {}
        }
    }

    pub(crate) fn handle_compose_key(&mut self, key: Key) {
        match key {
            Key::Esc => {
                if self.ui.composer.autocomplete.open {
                    self.ui.composer.autocomplete.open = false;
                } else {
                    self.ui.mode = UiMode::Normal;
                    self.ui.composer.reset_input();
                }
            }
            Key::Enter => {
                if self.accept_autocomplete_if_incomplete() {
                    if self.commands.is_no_arg_command(&self.ui.composer.buffer) {
                        self.submit_composer();
                    }
                    return;
                }
                self.submit_composer();
            }
            Key::ShiftEnter => self.ui.composer.insert('\n'),
            Key::Tab => {
                if !self.accept_autocomplete_tab() {
                    self.ui.composer.autocomplete.next();
                }
                return;
            }
            Key::BackTab => {
                self.ui.composer.autocomplete.previous();
                return;
            }
            Key::Up if self.ui.composer.previous_history() => {
                self.update_completions();
                return;
            }
            Key::Down if self.ui.composer.next_history() => {
                self.update_completions();
                return;
            }
            Key::Down if self.ui.composer.autocomplete.open => {
                self.ui.composer.autocomplete.next();
                return;
            }
            Key::Up if self.ui.composer.autocomplete.open => {
                self.ui.composer.autocomplete.previous();
                return;
            }
            Key::Backspace => self.ui.composer.backspace(),
            Key::Delete => self.ui.composer.delete(),
            Key::Left | Key::Ctrl('b') => self.ui.composer.move_left(),
            Key::Right | Key::Ctrl('f') => self.ui.composer.move_right(),
            Key::Alt('b') => self.ui.composer.move_word_left(),
            Key::Alt('f') => self.ui.composer.move_word_right(),
            Key::Ctrl('a') | Key::Home => self.ui.composer.cursor = 0,
            Key::Ctrl('e') | Key::End => self.ui.composer.cursor = self.ui.composer.buffer.len(),
            Key::Ctrl('u') => self.ui.composer.clear_before_cursor(),
            Key::Ctrl('k') => self.ui.composer.clear_after_cursor(),
            Key::Ctrl('w') => self.ui.composer.delete_word_before_cursor(),
            Key::Paste(text) => self.ui.composer.insert_str(&text.replace('\r', "\n")),
            Key::Char(ch) => self.ui.composer.insert(ch),
            _ => {}
        }
        self.update_completions();
    }

    pub(crate) fn handle_palette_key(&mut self, key: Key) {
        match key {
            Key::Esc => self.ui.mode = UiMode::Normal,
            Key::Enter | Key::ShiftEnter => self.run_palette_selection(),
            Key::Down | Key::Ctrl('n') => self.ui.palette.next(),
            Key::Up | Key::Ctrl('p') => self.ui.palette.previous(),
            Key::Backspace => {
                self.ui.palette.query.pop();
                self.rebuild_palette();
            }
            Key::Ctrl('u') => {
                self.ui.palette.query.clear();
                self.rebuild_palette();
            }
            Key::Char(ch) if !ch.is_control() => {
                self.ui.palette.query.push(ch);
                self.rebuild_palette();
            }
            _ => {}
        }
    }

    pub(crate) fn handle_prompt_key(&mut self, key: Key) {
        match key {
            Key::Esc => self.ui.mode = UiMode::Normal,
            Key::Enter | Key::ShiftEnter => {
                let value = self.ui.prompt.input.trim().to_string();
                let prefix = self.ui.prompt.prefix.clone();
                self.ui.mode = UiMode::Normal;
                self.dispatch_command_line(format!("{prefix}{value}"));
            }
            Key::Backspace => {
                self.ui.prompt.input.pop();
            }
            Key::Ctrl('u') => self.ui.prompt.input.clear(),
            Key::Paste(text) => self.ui.prompt.input.push_str(text.trim()),
            Key::Char(ch) if !ch.is_control() => self.ui.prompt.input.push(ch),
            _ => {}
        }
    }

    pub(crate) fn handle_help_key(&mut self, key: Key) {
        if matches!(
            key,
            Key::Esc | Key::Enter | Key::ShiftEnter | Key::Char('?') | Key::Char('q')
        ) {
            self.ui.mode = UiMode::Normal;
        }
    }

    pub(crate) fn handle_confirm_quit_key(&mut self, key: Key) {
        match key {
            Key::Char('y') | Key::Char('Y') | Key::Enter | Key::ShiftEnter => self.running = false,
            Key::Esc | Key::Char('n') | Key::Char('N') | Key::Char('q') => {
                self.ui.mode = UiMode::Normal
            }
            _ => {}
        }
    }
}
