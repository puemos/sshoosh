mod commands;
mod input;
mod render;
mod state;
mod theme;

use ratatui::layout::{Position, Rect};

use crate::{
    service::{Account, LiveEvent, ServerState, Snapshot},
    terminal::{self, SharedBuffer, SshooshTerminal},
};

use self::{
    commands::{CommandExecutor, CommandRegistry},
    input::{InputDecoder, Key, MouseButton, MouseEvent, MouseEventKind},
    state::{
        ActivePane, Banner, BottomBarAction, ComposerState, HitRegion, HitTarget, PaletteState,
        PromptState, Route, UiMode, UiState,
    },
};

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Action {
    CreateInvite,
    AcceptInvite { code: String, username: String },
    CreateChannel { name: String, private: bool },
    JoinChannel { slug: String },
    CreateThread { title: String, body: String },
    AddComment { body: String },
    OpenDm { target: String },
    SendDm { body: String },
    MarkThreadRead,
    MarkThreadUnread,
    MarkDmRead,
    MarkDmUnread,
    NextUnread,
}

pub struct App {
    pub running: bool,
    terminal: SshooshTerminal,
    shared: SharedBuffer,
    pub account: Account,
    state: ServerState,
    live_rx: tokio::sync::broadcast::Receiver<LiveEvent>,
    snapshot: Snapshot,
    ui: UiState,
    commands: CommandRegistry,
    decoder: InputDecoder,
    actions: Vec<Action>,
    refresh_requested: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
enum WorkspaceRow {
    Channel(String),
    Thread(String),
    Dm(String),
}

impl App {
    pub async fn new(
        account: Account,
        state: ServerState,
        cols: u16,
        rows: u16,
    ) -> anyhow::Result<Self> {
        let (terminal, shared) = terminal::terminal(cols.max(80), rows.max(24))?;
        let live_rx = state.subscribe();
        let snapshot = state.snapshot(&account.id, None, None, None).await?;
        let mut ui = UiState::default();
        ui.sync_route_from_snapshot(&snapshot);
        Ok(Self {
            running: true,
            terminal,
            shared,
            account,
            state,
            live_rx,
            snapshot,
            ui,
            commands: CommandRegistry::default(),
            decoder: InputDecoder::default(),
            actions: Vec::new(),
            refresh_requested: false,
        })
    }

    pub async fn refresh(&mut self) -> anyhow::Result<()> {
        self.account = self.state.reload_account(&self.account.id).await?;
        self.snapshot = self
            .state
            .snapshot(
                &self.account.id,
                self.snapshot.selected_channel_id.as_deref(),
                self.snapshot.selected_thread_id.as_deref(),
                self.snapshot.selected_conversation_id.as_deref(),
            )
            .await?;
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

    pub fn set_banner_ok(&mut self, text: impl Into<String>) {
        self.ui.banner = Some(Banner::ok(text));
    }

    pub fn set_banner_modal_ok(&mut self, text: impl Into<String>) {
        self.ui.banner = Some(Banner::modal_ok(text));
    }

    pub fn set_banner_err(&mut self, text: impl Into<String>) {
        self.ui.banner = Some(Banner::err(text));
    }

    pub fn selected_channel_id(&self) -> Option<String> {
        self.snapshot.selected_channel_id.clone()
    }

    pub fn selected_thread_id(&self) -> Option<String> {
        self.snapshot.selected_thread_id.clone()
    }

    pub fn selected_conversation_id(&self) -> Option<String> {
        self.snapshot.selected_conversation_id.clone()
    }

    pub fn select_channel(&mut self, channel_id: String) {
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
        self.snapshot.selected_channel_id = Some(channel_id.clone());
        self.snapshot.selected_thread_id = Some(thread_id);
        self.snapshot.selected_conversation_id = None;
        self.reset_detail_scroll();
        self.ui.route = Route::Channel(channel_id);
        self.ui.active_pane = ActivePane::Detail;
        self.ui.threads_collapsed = false;
        self.refresh_requested = true;
    }

    pub fn select_thread_at_bottom(&mut self, channel_id: String, thread_id: String) {
        self.select_thread(channel_id, thread_id);
        self.scroll_detail_to_bottom();
    }

    pub fn select_conversation(&mut self, conversation_id: String) {
        self.snapshot.selected_conversation_id = Some(conversation_id);
        self.reset_detail_scroll();
        self.ui.route = Route::Dms;
        self.ui.active_pane = ActivePane::Detail;
        self.refresh_requested = true;
    }

    pub fn select_conversation_at_bottom(&mut self, conversation_id: String) {
        self.select_conversation(conversation_id);
        self.scroll_detail_to_bottom();
    }

    pub fn handle_input(&mut self, bytes: &[u8]) {
        let keys = self.decoder.push(bytes);
        for key in keys {
            self.handle_key(key);
            if !self.running {
                break;
            }
        }
    }

    fn handle_key(&mut self, key: Key) {
        if let Key::Mouse(mouse) = key {
            self.handle_mouse(mouse);
            return;
        }

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
            if matches!(key, Key::Esc | Key::Enter | Key::ShiftEnter) {
                self.ui.banner = None;
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

    fn handle_mouse(&mut self, mouse: MouseEvent) {
        if self
            .ui
            .banner
            .as_ref()
            .is_some_and(|banner| banner.modal_active())
        {
            if matches!(mouse.kind, MouseEventKind::Down(MouseButton::Left)) {
                self.ui.banner = None;
            }
            return;
        }

        let Some(region) = self.ui.hit_map.hit(mouse.column, mouse.row) else {
            return;
        };

        match mouse.kind {
            MouseEventKind::ScrollUp => self.handle_mouse_scroll(&region.target, -3),
            MouseEventKind::ScrollDown => self.handle_mouse_scroll(&region.target, 3),
            MouseEventKind::ScrollLeft | MouseEventKind::ScrollRight => {}
            MouseEventKind::Down(MouseButton::Left) => self.handle_mouse_click(region, mouse),
            MouseEventKind::Drag(MouseButton::Left) => {
                self.handle_mouse_drag(&region.target, mouse)
            }
            MouseEventKind::Down(_)
            | MouseEventKind::Up(_)
            | MouseEventKind::Drag(_)
            | MouseEventKind::Moved => {}
        }
    }

    fn handle_mouse_scroll(&mut self, target: &HitTarget, delta: isize) {
        match target {
            HitTarget::WorkspaceScroll
            | HitTarget::WorkspaceChannel(_)
            | HitTarget::WorkspaceThread(_)
            | HitTarget::WorkspaceDm(_) => self.move_workspace(delta),
            HitTarget::DetailScroll => self.move_detail(delta),
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

    fn handle_mouse_drag(&mut self, target: &HitTarget, mouse: MouseEvent) {
        if matches!(target, HitTarget::ComposerInput { .. }) {
            if let Some(region) = self.ui.hit_map.hit(mouse.column, mouse.row) {
                self.handle_mouse_click(region, mouse);
            }
        }
    }

    fn handle_mouse_click(&mut self, region: HitRegion, mouse: MouseEvent) {
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
            HitTarget::BannerModal => self.ui.banner = None,
            HitTarget::ConfirmQuitYes => self.running = false,
            HitTarget::ConfirmQuitNo | HitTarget::ConfirmQuitBackdrop => {
                self.ui.mode = UiMode::Normal;
            }
            HitTarget::BottomBar(action) => self.run_bottom_bar_action(action),
        }
    }

    fn place_composer_cursor(&mut self, rect: Rect, scroll_y: u16, mouse: MouseEvent) {
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

    fn run_bottom_bar_action(&mut self, action: BottomBarAction) {
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
                    self.ui.composer = ComposerState::default();
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

    fn handle_onboarding_key(&mut self, key: Key) {
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

    fn handle_normal_key(&mut self, key: Key) {
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
            Key::Char('t') => self.enter_compose("/thread "),
            Key::Char('d') => self.enter_compose("/dm "),
            Key::Char('c') => self.enter_compose("/channel "),
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

    fn handle_compose_key(&mut self, key: Key) {
        match key {
            Key::Esc => {
                if self.ui.composer.autocomplete.open {
                    self.ui.composer.autocomplete.open = false;
                } else {
                    self.ui.mode = UiMode::Normal;
                    self.ui.composer = ComposerState::default();
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
            }
            Key::BackTab => self.ui.composer.autocomplete.previous(),
            Key::Down => self.ui.composer.autocomplete.next(),
            Key::Up => self.ui.composer.autocomplete.previous(),
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

    fn handle_palette_key(&mut self, key: Key) {
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

    fn handle_prompt_key(&mut self, key: Key) {
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

    fn handle_help_key(&mut self, key: Key) {
        if matches!(
            key,
            Key::Esc | Key::Enter | Key::ShiftEnter | Key::Char('?') | Key::Char('q')
        ) {
            self.ui.mode = UiMode::Normal;
        }
    }

    fn handle_confirm_quit_key(&mut self, key: Key) {
        match key {
            Key::Char('y') | Key::Char('Y') | Key::Enter | Key::ShiftEnter => self.running = false,
            Key::Esc | Key::Char('n') | Key::Char('N') | Key::Char('q') => {
                self.ui.mode = UiMode::Normal
            }
            _ => {}
        }
    }

    fn enter_compose(&mut self, initial: &str) {
        self.ui.mode = UiMode::Compose;
        self.ui.composer = ComposerState::from(initial);
        self.update_completions();
    }

    fn open_prompt(&mut self, title: &str, prefix: &str, placeholder: &str) {
        self.ui.mode = UiMode::Prompt;
        self.ui.prompt = PromptState {
            title: title.to_string(),
            prefix: prefix.to_string(),
            placeholder: placeholder.to_string(),
            input: String::new(),
        };
    }

    fn open_palette(&mut self) {
        self.ui.mode = UiMode::Palette;
        self.ui.palette = PaletteState::default();
        self.rebuild_palette();
    }

    fn rebuild_palette(&mut self) {
        self.ui.palette.items = self.commands.palette_items(&self.snapshot);
        let query = self.ui.palette.query.clone();
        self.ui.palette.apply_filter(&query);
    }

    fn run_palette_selection(&mut self) {
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
            } => self.open_prompt(title, prefix, placeholder),
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

    fn update_completions(&mut self) {
        self.ui.composer.autocomplete = self.commands.autocomplete(
            &self.ui.composer.buffer,
            self.ui.composer.cursor,
            &self.snapshot,
        );
    }

    fn accept_autocomplete_if_incomplete(&mut self) -> bool {
        let replacement = self.ui.composer.autocomplete.selected_replacement();
        if let Some((range, value)) = replacement {
            self.ui.composer.replace_range(range, &value);
            self.update_completions();
            return true;
        }
        false
    }

    fn accept_autocomplete_tab(&mut self) -> bool {
        let replacement = self.ui.composer.autocomplete.selected_tab_replacement();
        if let Some((range, value)) = replacement {
            self.ui.composer.replace_range(range, &value);
            self.update_completions();
            return true;
        }
        false
    }

    fn submit_onboarding(&mut self) {
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
        let username = parts.next().unwrap_or(&self.account.username).to_string();
        self.actions.push(Action::AcceptInvite { code, username });
    }

    fn submit_composer(&mut self) {
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

    fn dispatch_command_line(&mut self, line: String) {
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

    fn move_selection(&mut self, delta: isize) {
        if self.ui.active_pane == ActivePane::Detail {
            self.move_detail(delta);
        } else {
            self.move_workspace(delta);
        }
    }

    fn move_to_edge(&mut self, end: bool) {
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

    fn move_detail(&mut self, delta: isize) {
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

    fn reset_detail_scroll(&mut self) {
        self.ui.detail_scroll.scroll_to_top();
    }

    fn scroll_detail_to_bottom(&mut self) {
        self.ui
            .detail_scroll
            .set_offset(Position { x: 0, y: u16::MAX });
    }

    fn workspace_rows(&self) -> Vec<WorkspaceRow> {
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

    fn current_workspace_row(&self) -> Option<WorkspaceRow> {
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

    fn move_workspace(&mut self, delta: isize) {
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

    fn apply_workspace_row(&mut self, row: WorkspaceRow) {
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

    fn activate_selection(&mut self) {
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
            }
        }
    }

    fn navigate_left(&mut self) {
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

    fn navigate_right(&mut self) {
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

    fn toggle_workspace_detail(&mut self) {
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
        }
    }

    fn toggle_threads(&mut self) {
        if matches!(self.ui.route, Route::Channel(_)) {
            self.ui.threads_collapsed = !self.ui.threads_collapsed;
            if self.ui.threads_collapsed {
                self.ui.active_pane = ActivePane::Rail;
            }
        }
    }

    pub fn resize(&mut self, cols: u16, rows: u16) -> anyhow::Result<()> {
        self.terminal
            .resize(Rect::new(0, 0, cols.max(1), rows.max(1)))?;
        Ok(())
    }

    pub fn force_full_repaint(&mut self) {
        let _ = self.terminal.clear();
    }

    pub fn render(&mut self) -> anyhow::Result<Vec<u8>> {
        let account = &self.account;
        let snapshot = &self.snapshot;
        let ui = &mut self.ui;
        let commands = self.commands.specs();
        self.terminal.draw(|frame| {
            render::draw(frame, account, snapshot, ui, commands);
        })?;
        Ok(self.shared.take())
    }
}

fn clamp_index(current: usize, delta: isize, len: usize) -> usize {
    if len == 0 {
        return 0;
    }
    let next = current as isize + delta;
    next.clamp(0, len.saturating_sub(1) as isize) as usize
}

fn cursor_for_display_position(
    buffer: &str,
    width: usize,
    target_line: usize,
    target_col: usize,
) -> usize {
    let width = width.max(1);
    let mut line = 0;
    let mut col = 0;

    for (idx, ch) in buffer.char_indices() {
        if ch == '\n' {
            if line == target_line {
                return idx;
            }
            line += 1;
            col = 0;
            continue;
        }

        if col >= width {
            if line == target_line {
                return idx;
            }
            line += 1;
            col = 0;
        }

        if line == target_line && col >= target_col {
            return idx;
        }
        col += 1;
    }

    buffer.len()
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use uuid::Uuid;

    use crate::{
        db::Database,
        service::{Channel, Conversation, ServerState, Snapshot, ThreadItem},
    };

    use super::*;

    fn temp_path(name: &str) -> PathBuf {
        std::env::temp_dir().join(format!("sshoosh-app-{name}-{}", Uuid::now_v7()))
    }

    async fn test_app(name: &str) -> App {
        let db_path = temp_path(name).with_extension("sqlite");
        let db = Database::connect(&db_path).await.expect("connect db");
        db.init().await.expect("init db");
        let state = ServerState::new(db).await.expect("state");
        let account = state
            .ensure_account_for_key(
                "owner",
                &format!("SHA256:{name}"),
                &format!("ssh-ed25519 {name}"),
            )
            .await
            .expect("account");
        let mut app = App::new(account, state, 100, 30).await.expect("app");
        app.snapshot = snapshot();
        app.ui.route = Route::Channel("general".to_string());
        app
    }

    fn snapshot() -> Snapshot {
        Snapshot {
            current_username: Some("owner".to_string()),
            channels: vec![Channel {
                id: "general".to_string(),
                slug: "general".to_string(),
                name: "general".to_string(),
                visibility: "public".to_string(),
                topic: None,
                unread_count: 0,
            }],
            threads: vec![ThreadItem {
                id: "thread".to_string(),
                channel_id: "general".to_string(),
                title: "Deploy notes".to_string(),
                body: "Original post".to_string(),
                author: "owner".to_string(),
                comment_count: 0,
                last_comment_index: 1,
                unread_count: 0,
                last_activity_at: None,
                created_at: "2020-01-02T03:04:00Z".to_string(),
            }],
            conversations: vec![Conversation {
                id: "dm".to_string(),
                peer_username: "alice".to_string(),
                last_message_index: 0,
                unread_count: 0,
                last_activity_at: None,
                last_message_preview: None,
            }],
            selected_channel_id: Some("general".to_string()),
            selected_thread_id: Some("thread".to_string()),
            ..Snapshot::default()
        }
    }

    fn click_region(app: &mut App, target: impl Fn(&HitTarget) -> bool) {
        let region = app
            .ui
            .hit_map
            .entries()
            .iter()
            .find(|region| target(&region.target))
            .cloned()
            .expect("hit region");
        click_at(app, region.rect.x, region.rect.y);
    }

    fn click_at(app: &mut App, column: u16, row: u16) {
        app.handle_input(format!("\x1b[<0;{};{}M", column + 1, row + 1).as_bytes());
    }

    #[tokio::test]
    async fn mouse_clicks_workspace_thread_and_dm_rows() {
        let mut app = test_app("workspace-clicks").await;
        app.render().expect("render");

        click_region(
            &mut app,
            |target| matches!(target, HitTarget::WorkspaceThread(id) if id == "thread"),
        );
        assert_eq!(app.snapshot.selected_thread_id.as_deref(), Some("thread"));
        assert_eq!(app.ui.active_pane, ActivePane::Detail);

        app.render().expect("render");
        click_region(
            &mut app,
            |target| matches!(target, HitTarget::WorkspaceDm(id) if id == "dm"),
        );
        assert_eq!(app.snapshot.selected_conversation_id.as_deref(), Some("dm"));
        assert_eq!(app.ui.route, Route::Dms);
        assert_eq!(app.ui.active_pane, ActivePane::Detail);
    }

    #[tokio::test]
    async fn mouse_places_composer_cursor_and_accepts_autocomplete() {
        let mut app = test_app("composer-clicks").await;
        app.ui.mode = UiMode::Compose;
        app.ui.composer = ComposerState::from("hello\nworld");
        app.render().expect("render");
        let input = app
            .ui
            .hit_map
            .entries()
            .iter()
            .find(|region| matches!(region.target, HitTarget::ComposerInput { .. }))
            .cloned()
            .expect("composer input");
        click_at(&mut app, input.rect.x + 3, input.rect.y + 1);
        assert_eq!(app.ui.composer.cursor, 9);

        app.ui.composer = ComposerState::from("/");
        app.update_completions();
        app.render().expect("render");
        click_region(&mut app, |target| {
            matches!(target, HitTarget::AutocompleteRow(0))
        });
        assert_eq!(app.ui.composer.buffer, "/invite");
        assert_eq!(app.ui.composer.cursor, app.ui.composer.buffer.len());
    }

    #[tokio::test]
    async fn mouse_runs_palette_and_closes_overlays() {
        let mut app = test_app("overlay-clicks").await;
        app.open_palette();
        app.render().expect("render");
        click_region(&mut app, |target| {
            matches!(target, HitTarget::PaletteRow(0))
        });
        assert_eq!(app.ui.mode, UiMode::Prompt);
        assert_eq!(app.ui.prompt.prefix, "/thread ");

        app.render().expect("render");
        click_at(&mut app, 0, 0);
        assert_eq!(app.ui.mode, UiMode::Normal);

        app.ui.mode = UiMode::Help;
        app.render().expect("render");
        click_at(&mut app, 0, 0);
        assert_eq!(app.ui.mode, UiMode::Normal);

        app.ui.mode = UiMode::ConfirmQuit;
        app.running = true;
        app.render().expect("render");
        click_region(&mut app, |target| {
            matches!(target, HitTarget::ConfirmQuitYes)
        });
        assert!(!app.running);
    }

    #[test]
    fn display_cursor_mapping_handles_wrapped_and_multiline_text() {
        assert_eq!(cursor_for_display_position("hello\nworld", 20, 1, 3), 9);
        assert_eq!(cursor_for_display_position("abcdef", 3, 1, 2), 5);
        assert_eq!(cursor_for_display_position("abc", 20, 3, 0), 3);
    }
}
