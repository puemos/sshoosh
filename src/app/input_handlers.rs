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
        self.ui.dismiss_startup_splash();

        if let Key::Mouse(mouse) = key {
            self.handle_mouse(mouse);
            return;
        }

        self.ui.selection.clear();

        if matches!(key, Key::Ctrl('c') | Key::Ctrl('d')) {
            self.running = false;
            return;
        }

        if self.ui.comment_delete.is_some() {
            self.handle_comment_delete_key(key);
            return;
        }

        if self.ui.comment_menu.is_some() && matches!(key, Key::Esc) {
            self.close_comment_overlays();
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
                    if let Some((kind, code)) = self
                        .active_modal_token()
                        .map(|(kind, code)| (kind, code.to_string()))
                    {
                        self.pending_clipboard_copy = Some(code);
                        self.ui.banner = Some(Banner::ok(format!("{kind} copied")));
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
            UiMode::Help => self.handle_help_key(key),
            UiMode::ConfirmQuit => self.handle_confirm_quit_key(key),
        }
    }

    pub(crate) fn handle_mouse(&mut self, mouse: MouseEvent) {
        self.ui.dismiss_startup_splash();

        if self
            .ui
            .banner
            .as_ref()
            .is_some_and(|banner| banner.modal_active())
        {
            self.ui.selection.clear();
            if matches!(mouse.kind, MouseEventKind::Down(MouseButton::Left))
                && let Some(region) = self.ui.hit_map.hit(mouse.column, mouse.row)
                && matches!(
                    region.target,
                    HitTarget::ListModalRow(_) | HitTarget::BannerModal
                )
            {
                self.handle_mouse_click(region, mouse);
            }
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
            MouseEventKind::Down(MouseButton::Right) => self.open_comment_menu_at(mouse),
            MouseEventKind::Down(_) => self.ui.selection.clear(),
            MouseEventKind::Moved => self.update_pointer_shape(mouse),
            MouseEventKind::Up(_) | MouseEventKind::Drag(_) => {}
        }
    }

    pub(crate) fn update_pointer_shape(&mut self, mouse: MouseEvent) {
        self.desired_pointer_shape = match self.ui.hit_map.hit(mouse.column, mouse.row) {
            Some(HitRegion {
                target:
                    HitTarget::MessageLink(_)
                    | HitTarget::ReactionChip { .. }
                    | HitTarget::ReactionAdd { .. }
                    | HitTarget::SearchResult(_)
                    | HitTarget::LabelResult(_)
                    | HitTarget::SavedResult(_)
                    | HitTarget::NotificationResult(_)
                    | HitTarget::NotificationFilter(_)
                    | HitTarget::NotificationReadAll
                    | HitTarget::NotificationArchiveAll
                    | HitTarget::TopbarMentions
                    | HitTarget::ListModalRow(_)
                    | HitTarget::WorkspaceLabel(_)
                    | HitTarget::WorkspaceLabelsMore
                    | HitTarget::MessageLabel(_),
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
            | HitTarget::WorkspaceLabel(_)
            | HitTarget::WorkspaceLabelsMore
            | HitTarget::WorkspaceSaved
            | HitTarget::WorkspaceNotifications
            | HitTarget::WorkspaceDm { .. } => self.move_workspace(delta),
            HitTarget::DetailScroll
            | HitTarget::SearchResult(_)
            | HitTarget::LabelResult(_)
            | HitTarget::SavedResult(_)
            | HitTarget::NotificationResult(_)
            | HitTarget::NotificationFilter(_)
            | HitTarget::NotificationReadAll
            | HitTarget::NotificationArchiveAll
            | HitTarget::EditableMessage(_)
            | HitTarget::ReactionChip { .. }
            | HitTarget::ReactionAdd { .. }
            | HitTarget::MessageLink(_)
            | HitTarget::MessageLabel(_) => self.move_detail(delta),
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
            HitTarget::HelpScroll => {
                let steps = delta.unsigned_abs().max(1);
                for _ in 0..steps {
                    if delta < 0 {
                        self.ui.help_scroll.scroll_up();
                    } else {
                        self.ui.help_scroll.scroll_down();
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
            message_region: self
                .ui
                .message_selection_regions
                .iter()
                .rev()
                .copied()
                .find(|region| region.contains(mouse.column, mouse.row)),
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
        self.ui.selection.message_region = anchor.message_region;
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
            self.ui.selection.message_region = anchor.message_region;
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
            HitTarget::TopbarMentions => self.actions.push(Action::ListMentions),
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
            HitTarget::WorkspaceLabel(tag) => self.apply_workspace_row(WorkspaceRow::Label(tag)),
            HitTarget::WorkspaceLabelsMore => self.apply_workspace_row(WorkspaceRow::LabelsMore),
            HitTarget::WorkspaceSaved => self.apply_workspace_row(WorkspaceRow::Saved),
            HitTarget::WorkspaceNotifications => {
                self.apply_workspace_row(WorkspaceRow::Notifications)
            }
            HitTarget::WorkspaceDm {
                conversation_id: Some(conversation_id),
                ..
            } => self.select_conversation(conversation_id),
            HitTarget::WorkspaceDm {
                conversation_id: None,
                username,
            } => self.actions.push(Action::OpenDm { target: username }),
            HitTarget::WorkspaceScroll => self.ui.active_pane = ActivePane::Rail,
            HitTarget::DetailScroll => {
                self.ui.active_pane = ActivePane::Detail;
                self.activate_result_at_mouse_position(region.rect, mouse);
            }
            HitTarget::SearchResult(index) => {
                self.ui.active_pane = ActivePane::Detail;
                self.ui.search_selected = index;
                self.activate_search_result();
            }
            HitTarget::LabelResult(index) => {
                self.ui.active_pane = ActivePane::Detail;
                self.ui.label_selected = index;
                self.activate_label_result();
            }
            HitTarget::SavedResult(index) => {
                self.ui.active_pane = ActivePane::Detail;
                self.ui.saved_selected = index;
                self.activate_saved_result();
            }
            HitTarget::NotificationResult(index) => {
                self.ui.active_pane = ActivePane::Detail;
                self.ui.notifications_selected = index;
                self.activate_notification_result();
            }
            HitTarget::NotificationFilter(filter) => {
                self.ui.active_pane = ActivePane::Detail;
                self.set_notification_filter(filter);
            }
            HitTarget::NotificationReadAll => {
                self.ui.active_pane = ActivePane::Detail;
                self.actions.push(Action::MarkNotificationRead {
                    notification_id: None,
                });
            }
            HitTarget::NotificationArchiveAll => {
                self.ui.active_pane = ActivePane::Detail;
                self.actions.push(Action::ArchiveNotifications);
            }
            HitTarget::EditableMessage(_) => self.ui.active_pane = ActivePane::Detail,
            HitTarget::ReactionChip {
                target,
                emoji,
                reacted_by_me,
            } => {
                self.ui.active_pane = ActivePane::Detail;
                let index = target.index();
                if reacted_by_me {
                    self.actions.push(Action::Unreact { emoji, index });
                } else {
                    self.actions.push(Action::React { emoji, index });
                }
            }
            HitTarget::ReactionAdd { target } => {
                self.ui.active_pane = ActivePane::Detail;
                self.prefill_reaction_add(target);
            }
            HitTarget::MessageLink(url) => {
                self.ui.active_pane = ActivePane::Detail;
                self.pending_link_open = Some(url);
            }
            HitTarget::MessageLabel(tag) => {
                self.ui.active_pane = ActivePane::Detail;
                self.actions.push(Action::OpenLabel { tag });
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
            HitTarget::AutocompleteScroll | HitTarget::HelpScroll => {}
            HitTarget::PaletteRow(row) => {
                if row < self.ui.palette.filtered.len() {
                    self.ui.palette.selected = row;
                    self.run_palette_selection();
                }
            }
            HitTarget::PaletteInput | HitTarget::PaletteResults => {}
            HitTarget::PaletteBackdrop | HitTarget::HelpBackdrop => self.ui.mode = UiMode::Normal,
            HitTarget::BannerModal => {}
            HitTarget::ListModalRow(row) => {
                let action = self
                    .ui
                    .banner
                    .as_ref()
                    .and_then(|banner| banner.list.as_ref())
                    .and_then(|list| list.row_actions.get(row))
                    .cloned()
                    .flatten();
                if let Some(ListModalAction::OpenSource(target)) = action {
                    self.ui.banner = None;
                    self.actions.push(Action::OpenSourceTarget { target });
                }
            }
            HitTarget::ConfirmQuitYes => self.running = false,
            HitTarget::ConfirmQuitNo | HitTarget::ConfirmQuitBackdrop => {
                self.ui.mode = UiMode::Normal;
            }
            HitTarget::BottomBar(action) => self.run_bottom_bar_action(action),
            HitTarget::CommentMenuBackdrop => self.close_comment_overlays(),
            HitTarget::CommentMenuEdit(target) => {
                self.prefill_message_edit(target);
            }
            HitTarget::CommentMenuDelete(target) => {
                self.ui.comment_menu = None;
                self.ui.comment_delete = Some(CommentDeleteState { target });
            }
            HitTarget::CommentMenuSave { target, saved } => {
                self.close_comment_overlays();
                self.actions.push(Action::SetMessageSaved {
                    index: target.index(),
                    saved,
                });
            }
            HitTarget::CommentDeleteConfirm(target) => {
                self.close_comment_overlays();
                match target {
                    EditableMessageTarget::Comment(index) => {
                        self.actions.push(Action::DeleteComment { index });
                    }
                    EditableMessageTarget::Dm(index) => {
                        self.actions.push(Action::DeleteDm { index });
                    }
                }
            }
            HitTarget::CommentDeleteCancel => self.close_comment_overlays(),
        }
    }

    pub(crate) fn activate_result_at_mouse_position(&mut self, _rect: Rect, mouse: MouseEvent) {
        if !matches!(
            self.ui.route,
            Route::Search | Route::Label(_) | Route::Saved | Route::Notifications
        ) {
            return;
        }

        let Some(region) = self.ui_hit_row_matching_for_route(mouse.row) else {
            return;
        };
        match region.target {
            HitTarget::SearchResult(index) if index < self.snapshot.search_results.len() => {
                self.ui.search_selected = index;
                self.activate_search_result();
            }
            HitTarget::LabelResult(index) if index < self.snapshot.label_items.len() => {
                self.ui.label_selected = index;
                self.activate_label_result();
            }
            HitTarget::SavedResult(index) if index < self.snapshot.saved_messages.len() => {
                self.ui.saved_selected = index;
                self.activate_saved_result();
            }
            HitTarget::NotificationResult(index)
                if index < self.visible_notification_indices().len() =>
            {
                self.ui.notifications_selected = index;
                self.activate_notification_result();
            }
            _ => {}
        }
    }

    fn ui_hit_target_matches_route(&self, target: &HitTarget) -> bool {
        matches!(
            (&self.ui.route, target),
            (Route::Search, HitTarget::SearchResult(_))
                | (Route::Label(_), HitTarget::LabelResult(_))
                | (Route::Saved, HitTarget::SavedResult(_))
                | (Route::Notifications, HitTarget::NotificationResult(_))
        )
    }

    fn ui_hit_row_matching_for_route(&self, row: u16) -> Option<HitRegion> {
        self.ui
            .hit_map
            .hit_row_matching(row, |target| self.ui_hit_target_matches_route(target))
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
            BottomBarAction::OpenHelp => self.open_help(),
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
            Key::Paste(text) => self.ui.composer.insert_str(&sanitize_composer_paste(&text)),
            Key::Char(ch) if !ch.is_control() => self.ui.composer.insert(ch),
            _ => {}
        }
    }

    pub(crate) fn handle_normal_key(&mut self, key: Key) {
        match key {
            Key::Char('q') => self.ui.mode = UiMode::ConfirmQuit,
            Key::Char('?') | Key::CtrlSeq('x', 'h') => self.open_help(),
            Key::Ctrl('p') | Key::CtrlSeq('x', 'p') => self.open_palette(),
            Key::Tab | Key::BackTab => self.toggle_workspace_detail(),
            Key::Left | Key::Char('h') => self.navigate_left(),
            Key::Right | Key::Char('l') => self.navigate_right(),
            Key::Down | Key::Char('j') => self.move_selection(1),
            Key::Up | Key::Char('k') => self.move_selection(-1),
            Key::PageDown if self.ui.active_pane == ActivePane::Detail => {
                self.page_detail(true);
            }
            Key::PageUp if self.ui.active_pane == ActivePane::Detail => {
                self.page_detail(false);
            }
            Key::PageDown => self.move_selection(8),
            Key::PageUp => self.move_selection(-8),
            Key::Home | Key::Char('g') => self.move_to_edge(false),
            Key::End | Key::Char('G') => self.move_to_edge(true),
            Key::Enter | Key::ShiftEnter => self.activate_selection(),
            Key::Char('f') if self.ui.route == Route::Notifications => {
                self.cycle_notification_filter();
            }
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
                if self.accept_autocomplete_enter() {
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
            Key::CtrlSeq('x', 'e') | Key::CtrlSeq('x', 'E') => {
                self.prefill_last_own_comment_edit();
                return;
            }
            Key::Paste(text) => self.ui.composer.insert_str(&sanitize_composer_paste(&text)),
            Key::Char(ch) if !ch.is_control() => self.ui.composer.insert(ch),
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

    pub(crate) fn handle_help_key(&mut self, key: Key) {
        match key {
            Key::Esc | Key::Enter | Key::ShiftEnter | Key::Char('?') | Key::Char('q') => {
                self.ui.mode = UiMode::Normal;
            }
            Key::Down | Key::Char('j') => self.ui.help_scroll.scroll_down(),
            Key::Up | Key::Char('k') => self.ui.help_scroll.scroll_up(),
            Key::PageDown => self.ui.help_scroll.scroll_page_down(),
            Key::PageUp => self.ui.help_scroll.scroll_page_up(),
            Key::Home | Key::Char('g') => self.ui.help_scroll.scroll_to_top(),
            Key::End | Key::Char('G') => self.ui.help_scroll.scroll_to_bottom(),
            _ => {}
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

    pub(crate) fn handle_comment_delete_key(&mut self, key: Key) {
        let Some(confirm) = self.ui.comment_delete else {
            return;
        };
        match key {
            Key::Char('y') | Key::Char('Y') | Key::Enter | Key::ShiftEnter => {
                self.close_comment_overlays();
                match confirm.target {
                    EditableMessageTarget::Comment(index) => {
                        self.actions.push(Action::DeleteComment { index });
                    }
                    EditableMessageTarget::Dm(index) => {
                        self.actions.push(Action::DeleteDm { index });
                    }
                }
            }
            Key::Esc | Key::Char('n') | Key::Char('N') | Key::Char('q') => {
                self.close_comment_overlays();
            }
            _ => {}
        }
    }

    pub(crate) fn open_comment_menu_at(&mut self, mouse: MouseEvent) {
        self.ui.selection.clear();
        let exact_region = self
            .ui
            .hit_map
            .hit_matching(mouse.column, mouse.row, |target| {
                matches!(target, HitTarget::EditableMessage(_))
            });
        let row_region = exact_region.or_else(|| {
            let hit = self.ui.hit_map.hit(mouse.column, mouse.row)?;
            if !matches!(
                hit.target,
                HitTarget::DetailScroll
                    | HitTarget::MessageLink(_)
                    | HitTarget::MessageLabel(_)
                    | HitTarget::ReactionChip { .. }
                    | HitTarget::ReactionAdd { .. }
            ) {
                return None;
            }
            self.ui.hit_map.hit_row_matching(mouse.row, |target| {
                matches!(target, HitTarget::EditableMessage(_))
            })
        });
        let Some(HitRegion {
            target: HitTarget::EditableMessage(target),
            ..
        }) = row_region
        else {
            self.close_comment_overlays();
            return;
        };
        self.ui.comment_delete = None;
        self.ui.comment_menu = Some(CommentMenuState {
            target,
            can_edit_delete: self.is_own_message(target),
            saved: self.is_message_saved(target),
            x: mouse.column,
            y: mouse.row,
        });
    }

    pub(crate) fn prefill_last_own_comment_edit(&mut self) {
        if matches!(self.ui.route, Route::Dms) {
            let Some(index) = self
                .snapshot
                .conversation_messages
                .iter()
                .rev()
                .find(|message| self.is_current_user(&message.author))
                .map(|message| message.obj_index)
            else {
                self.set_banner_err("No message by you in this DM");
                return;
            };
            self.prefill_message_edit(EditableMessageTarget::Dm(index));
            return;
        }

        let Some(index) = self
            .snapshot
            .comments
            .iter()
            .rev()
            .find(|comment| self.is_current_user(&comment.author))
            .map(|comment| comment.obj_index)
        else {
            self.set_banner_err("No comment by you in this thread");
            return;
        };
        self.prefill_message_edit(EditableMessageTarget::Comment(index));
    }

    pub(crate) fn prefill_message_edit(&mut self, target: EditableMessageTarget) {
        let Some(command) = self.message_edit_command(target) else {
            let label = match target {
                EditableMessageTarget::Comment(_) => "Comment",
                EditableMessageTarget::Dm(_) => "Message",
            };
            self.set_banner_err(format!("{label} is not editable"));
            return;
        };
        self.close_comment_overlays();
        self.ui.mode = UiMode::Compose;
        self.ui.composer = ComposerState::from(command.as_str());
        self.update_completions();
    }

    pub(crate) fn prefill_reaction_add(&mut self, target: ReactionTarget) {
        self.close_comment_overlays();
        self.ui.mode = UiMode::Compose;
        let prefix = "/reaction add :";
        let suffix = match target {
            ReactionTarget::ThreadRoot => "",
            ReactionTarget::Comment(index) | ReactionTarget::Dm(index) => {
                return self.prefill_reaction_add_with_suffix(prefix, &format!(" #{index}"));
            }
        };
        self.prefill_reaction_add_with_suffix(prefix, suffix);
    }

    fn prefill_reaction_add_with_suffix(&mut self, prefix: &str, suffix: &str) {
        self.ui.composer = ComposerState::from(format!("{prefix}{suffix}").as_str());
        self.ui.composer.cursor = prefix.len();
        self.update_completions();
    }

    pub(crate) fn message_edit_command(&self, target: EditableMessageTarget) -> Option<String> {
        match target {
            EditableMessageTarget::Comment(index) => {
                let comment = self
                    .snapshot
                    .comments
                    .iter()
                    .find(|comment| comment.obj_index == index)?;
                self.is_current_user(&comment.author)
                    .then(|| format!("/comment edit #{index} {}", comment.body))
            }
            EditableMessageTarget::Dm(index) => {
                let message = self
                    .snapshot
                    .conversation_messages
                    .iter()
                    .find(|message| message.obj_index == index)?;
                self.is_current_user(&message.author)
                    .then(|| format!("/dm edit #{index} {}", message.body))
            }
        }
    }

    pub(crate) fn is_own_message(&self, target: EditableMessageTarget) -> bool {
        match target {
            EditableMessageTarget::Comment(index) => self
                .snapshot
                .comments
                .iter()
                .find(|comment| comment.obj_index == index)
                .is_some_and(|comment| self.is_current_user(&comment.author)),
            EditableMessageTarget::Dm(index) => self
                .snapshot
                .conversation_messages
                .iter()
                .find(|message| message.obj_index == index)
                .is_some_and(|message| self.is_current_user(&message.author)),
        }
    }

    pub(crate) fn is_message_saved(&self, target: EditableMessageTarget) -> bool {
        match target {
            EditableMessageTarget::Comment(index) => self
                .snapshot
                .comments
                .iter()
                .find(|comment| comment.obj_index == index)
                .is_some_and(|comment| comment.saved_at.is_some()),
            EditableMessageTarget::Dm(index) => self
                .snapshot
                .conversation_messages
                .iter()
                .find(|message| message.obj_index == index)
                .is_some_and(|message| message.saved_at.is_some()),
        }
    }

    pub(crate) fn is_current_user(&self, author: &str) -> bool {
        self.snapshot
            .current_username
            .as_deref()
            .is_some_and(|username| username.eq_ignore_ascii_case(author))
    }

    pub(crate) fn close_comment_overlays(&mut self) {
        self.ui.comment_menu = None;
        self.ui.comment_delete = None;
    }
}
