mod commands;
mod input;
mod render;
mod state;
mod theme;

use ratatui::layout::{Position, Rect};

use crate::{
    service::{
        Account, DEFAULT_HISTORY_LIMIT, LiveEvent, MAX_HISTORY_LIMIT, Role, SearchResult,
        ServerState, Snapshot,
    },
    terminal::{self, SharedBuffer, SshooshTerminal},
};

const DEFAULT_SEARCH_LIMIT: i64 = 50;
const SEARCH_PAGE_SIZE: i64 = 50;
const MAX_SEARCH_LIMIT: i64 = 500;

use self::{
    commands::{CommandExecutor, CommandRegistry},
    input::{InputDecoder, Key, MouseButton, MouseEvent, MouseEventKind},
    state::{
        ActivePane, Banner, BottomBarAction, ComposerState, HitRegion, HitTarget, PaletteState,
        PromptState, Route, SelectionAnchor, SelectionRange, UiMode, UiState,
    },
};

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Action {
    CreateInvite,
    CreateInviteWithOptions {
        role: Role,
        ttl_hours: Option<i64>,
    },
    AcceptInvite {
        code: String,
        username: String,
    },
    CreateChannel {
        name: String,
        private: bool,
    },
    JoinChannel {
        slug: String,
    },
    LeaveChannel {
        slug: Option<String>,
    },
    ListChannels,
    RenameChannel {
        slug: Option<String>,
        name: String,
    },
    SetChannelTopic {
        slug: Option<String>,
        topic: String,
    },
    SetChannelArchived {
        slug: Option<String>,
        archived: bool,
    },
    CreateThread {
        title: String,
        body: String,
    },
    AddComment {
        body: String,
    },
    OpenDm {
        target: String,
    },
    SendDm {
        body: String,
    },
    MarkThreadRead,
    MarkThreadUnread,
    MarkDmRead,
    MarkDmUnread,
    NextUnread,
    ListUsers,
    SetUsername {
        username: String,
    },
    SetProfile {
        display_name: String,
    },
    SetUserDisabled {
        username: String,
        disabled: bool,
    },
    SetUserRole {
        username: String,
        role: Role,
    },
    ListKeys,
    ListMyKeys,
    AddKey {
        public_key: String,
        label: Option<String>,
    },
    LabelKey {
        key: String,
        label: String,
    },
    RevokeKey {
        key: String,
    },
    ListInvites,
    RevokeInvite {
        invite_id: String,
    },
    ListChannelMembers {
        slug: String,
    },
    AddChannelMember {
        slug: String,
        username: String,
    },
    RemoveChannelMember {
        slug: String,
        username: String,
    },
    RenameThread {
        title: String,
    },
    DeleteThread,
    SetThreadArchived {
        archived: bool,
    },
    SetThreadPinned {
        pinned: bool,
    },
    SetThreadMuted {
        ttl_hours: Option<i64>,
    },
    SetThreadSaved {
        saved: bool,
    },
    EditComment {
        index: i64,
        body: String,
    },
    DeleteComment {
        index: i64,
    },
    EditDm {
        index: i64,
        body: String,
    },
    DeleteDm {
        index: i64,
    },
    SetDmMuted {
        ttl_hours: Option<i64>,
    },
    SetDmSaved {
        saved: bool,
    },
    React {
        emoji: String,
        index: Option<i64>,
    },
    Unreact {
        emoji: String,
        index: Option<i64>,
    },
    ListMentions,
    ListNotifications,
    MarkNotificationRead {
        notification_id: Option<String>,
    },
    ListWebhooks,
    AddWebhook {
        name: String,
        url: String,
    },
    RemoveWebhook {
        webhook_id: String,
    },
    ListAudit,
    Search {
        query: String,
    },
    LoadMore,
    LoadOlder,
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
    pending_link_open: Option<String>,
    pending_clipboard_copy: Option<String>,
    desired_pointer_shape: PointerShape,
    emitted_pointer_shape: PointerShape,
    history_limit: i64,
    search_limit: i64,
}

#[derive(Clone, Debug, PartialEq, Eq)]
enum WorkspaceRow {
    Channel(String),
    Thread(String),
    Dm(String),
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
enum PointerShape {
    #[default]
    Default,
    Pointer,
}

impl PointerShape {
    fn as_str(self) -> &'static str {
        match self {
            Self::Default => "default",
            Self::Pointer => "pointer",
        }
    }
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
            pending_link_open: None,
            pending_clipboard_copy: None,
            desired_pointer_shape: PointerShape::Default,
            emitted_pointer_shape: PointerShape::Default,
            history_limit: DEFAULT_HISTORY_LIMIT,
            search_limit: DEFAULT_SEARCH_LIMIT,
        })
    }

    pub async fn refresh(&mut self) -> anyhow::Result<()> {
        self.account = match self.state.reload_account(&self.account.id).await {
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
            .state
            .snapshot_with_history_limit(
                &self.account.id,
                self.snapshot.selected_channel_id.as_deref(),
                self.snapshot.selected_thread_id.as_deref(),
                self.snapshot.selected_conversation_id.as_deref(),
                self.history_limit,
            )
            .await?;
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

    pub fn selected_thread_id(&self) -> Option<String> {
        self.snapshot.selected_thread_id.clone()
    }

    pub fn selected_conversation_id(&self) -> Option<String> {
        self.snapshot.selected_conversation_id.clone()
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

    fn handle_mouse(&mut self, mouse: MouseEvent) {
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

    fn update_pointer_shape(&mut self, mouse: MouseEvent) {
        self.desired_pointer_shape = match self.ui.hit_map.hit(mouse.column, mouse.row) {
            Some(HitRegion {
                target: HitTarget::MessageLink(_),
                ..
            }) => PointerShape::Pointer,
            _ => PointerShape::Default,
        };
    }

    fn handle_mouse_scroll(&mut self, target: &HitTarget, delta: isize) {
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

    fn start_mouse_click_or_selection(&mut self, mouse: MouseEvent) {
        self.ui.selection.clear();
        self.ui.selection.pending = Some(SelectionAnchor {
            at: mouse_position(mouse),
            region: self.ui.hit_map.hit(mouse.column, mouse.row),
            modifiers: mouse.modifiers,
            moved: false,
        });
    }

    fn update_mouse_selection(&mut self, mouse: MouseEvent) {
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

    fn finish_mouse_click_or_selection(&mut self, mouse: MouseEvent) {
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
                return;
            }
            Key::BackTab => {
                self.ui.composer.autocomplete.previous();
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
            if self.ui.composer.buffer.get(range.clone()) == Some(value.as_str()) {
                return false;
            }
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

    fn move_search(&mut self, delta: isize) {
        let len = self.snapshot.search_results.len();
        if len == 0 {
            return;
        }
        self.ui.search_selected = clamp_index(self.ui.search_selected, delta, len);
    }

    fn reset_history_limit(&mut self) {
        self.history_limit = DEFAULT_HISTORY_LIMIT;
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

    fn activate_search_result(&mut self) {
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
            render::apply_selection(frame, ui);
        })?;
        let mut output = self.shared.take();
        for link in &self.ui.link_overlays {
            output.extend(terminal::osc8_hyperlink_at(
                link.rect, &link.url, &link.text, link.style,
            ));
        }
        if self.pending_link_open.take().is_some() {
            self.ui.banner = Some(Banner::ok(
                "Link is available through terminal hyperlink support",
            ));
        }
        if self.desired_pointer_shape != self.emitted_pointer_shape {
            output.extend(terminal::pointer_shape(self.desired_pointer_shape.as_str()));
            self.emitted_pointer_shape = self.desired_pointer_shape;
        }
        if self.ui.selection.copy_requested {
            self.ui.selection.copy_requested = false;
            if !self.ui.selection.text.is_empty() {
                output.extend(terminal::osc52_copy(&self.ui.selection.text));
                self.ui.banner = Some(Banner::ok("Selection copied"));
            }
            self.ui.selection.clear();
        }
        if let Some(text) = self.pending_clipboard_copy.take()
            && !text.is_empty()
        {
            output.extend(terminal::osc52_copy(&text));
        }
        Ok(output)
    }

    fn active_invite_code(&self) -> Option<&str> {
        self.ui
            .banner
            .as_ref()
            .filter(|banner| banner.modal_active())
            .and_then(|banner| banner.text.strip_prefix("Invite code:"))
            .map(str::trim)
            .filter(|code| !code.is_empty())
    }
}

fn mouse_position(mouse: MouseEvent) -> Position {
    Position {
        x: mouse.column,
        y: mouse.row,
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
                edited_at: None,
                archived_at: None,
                pinned_at: None,
                muted_until: None,
                saved_at: None,
                reactions: String::new(),
            }],
            conversations: vec![Conversation {
                id: "dm".to_string(),
                peer_username: "alice".to_string(),
                last_message_index: 0,
                unread_count: 0,
                last_activity_at: None,
                last_message_preview: None,
                muted_until: None,
                saved_at: None,
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
        app.handle_input(
            format!(
                "\x1b[<0;{};{}M\x1b[<0;{};{}m",
                column + 1,
                row + 1,
                column + 1,
                row + 1
            )
            .as_bytes(),
        );
    }

    fn move_at(app: &mut App, column: u16, row: u16) {
        app.handle_input(format!("\x1b[<35;{};{}M", column + 1, row + 1).as_bytes());
    }

    fn drag_at(app: &mut App, start: Position, end: Position) {
        app.handle_input(
            format!(
                "\x1b[<0;{};{}M\x1b[<32;{};{}M\x1b[<0;{};{}m",
                start.x + 1,
                start.y + 1,
                end.x + 1,
                end.y + 1,
                end.x + 1,
                end.y + 1
            )
            .as_bytes(),
        );
    }

    #[tokio::test]
    async fn arrow_keys_navigate_open_autocomplete() {
        let mut app = test_app("autocomplete-arrows").await;
        app.ui.mode = UiMode::Compose;
        app.ui.composer = ComposerState::from("/");
        app.update_completions();

        assert!(app.ui.composer.autocomplete.open);
        assert_eq!(app.ui.composer.autocomplete.selected, 0);

        app.handle_input(b"\x1b[B");
        assert_eq!(app.ui.composer.autocomplete.selected, 1);

        app.handle_input(b"\x1b[A");
        assert_eq!(app.ui.composer.autocomplete.selected, 0);
    }

    #[tokio::test]
    async fn invite_modal_c_copies_code_and_shows_toast() {
        let mut app = test_app("invite-copy").await;
        app.set_banner_modal_ok("Invite code: copy-me");

        app.handle_input(b"c");
        let output = app.render().expect("render copy");
        let output = String::from_utf8_lossy(&output);

        assert!(output.contains("\x1b]52;c;Y29weS1tZQ==\x07"), "{output:?}");
        assert!(output.contains("Invite code copied"), "{output:?}");
        assert_eq!(app.active_invite_code(), None);
    }

    #[tokio::test]
    async fn invite_modal_does_not_close_on_mouse_click() {
        let mut app = test_app("invite-click").await;
        app.set_banner_modal_ok("Invite code: stay-open");
        app.render().expect("render modal");

        click_region(&mut app, |target| matches!(target, HitTarget::BannerModal));

        assert_eq!(app.active_invite_code(), Some("stay-open"));
    }

    #[tokio::test]
    async fn mouse_clicks_workspace_thread_and_dm_rows() {
        let mut app = test_app("workspace-clicks").await;
        app.ui.active_pane = ActivePane::Rail;
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
    async fn link_text_is_hyperlinked_and_click_requests_open() {
        let mut app = test_app("link-clicks").await;
        app.snapshot.threads[0].body = "https://openai.com".to_string();
        app.ui.active_pane = ActivePane::Detail;

        let output = String::from_utf8_lossy(&app.render().expect("render")).into_owned();
        assert!(
            output.contains("\x1b]8;;https://openai.com\x1b\\https://openai.com\x1b]8;;\x1b\\"),
            "{output:?}"
        );
        let region = app
            .ui
            .hit_map
            .entries()
            .iter()
            .find(|region| {
                matches!(&region.target, HitTarget::MessageLink(url) if url == "https://openai.com")
            })
            .cloned()
            .expect("link hit region");

        click_at(&mut app, region.rect.x, region.rect.y);

        assert_eq!(app.pending_link_open.as_deref(), Some("https://openai.com"));
    }

    #[tokio::test]
    async fn mouse_hover_changes_pointer_shape_for_links() {
        let mut app = test_app("link-hover").await;
        app.snapshot.threads[0].body = "https://openai.com".to_string();
        app.ui.active_pane = ActivePane::Detail;
        app.render().expect("render");
        let region = app
            .ui
            .hit_map
            .entries()
            .iter()
            .find(|region| matches!(&region.target, HitTarget::MessageLink(_)))
            .cloned()
            .expect("link hit region");

        move_at(&mut app, region.rect.x, region.rect.y);
        let output = String::from_utf8_lossy(&app.render().expect("render pointer")).into_owned();
        assert!(output.contains("\x1b]22;pointer\x1b\\"), "{output:?}");

        move_at(&mut app, 0, 0);
        let output = String::from_utf8_lossy(&app.render().expect("render default")).into_owned();
        assert!(output.contains("\x1b]22;default\x1b\\"), "{output:?}");
    }

    #[tokio::test]
    async fn mouse_drag_selects_text_and_suppresses_click_action() {
        let mut app = test_app("drag-selects").await;
        app.ui.active_pane = ActivePane::List;
        app.render().expect("render");
        let thread_region = app
            .ui
            .hit_map
            .entries()
            .iter()
            .find(|region| matches!(region.target, HitTarget::WorkspaceThread(_)))
            .cloned()
            .expect("thread row");

        drag_at(
            &mut app,
            Position {
                x: thread_region.rect.x,
                y: thread_region.rect.y,
            },
            Position {
                x: thread_region.rect.x + 8,
                y: thread_region.rect.y,
            },
        );

        assert_eq!(app.ui.active_pane, ActivePane::List);
        assert!(app.ui.selection.range.is_some());
        assert!(app.ui.selection.copy_requested);
        let output =
            String::from_utf8_lossy(&app.render().expect("render after select")).into_owned();
        assert!(output.contains("\x1b]52;c;"), "{output:?}");
        assert!(app.ui.selection.range.is_none());
        assert!(app.ui.selection.text.is_empty());
        assert!(!app.ui.selection.copy_requested);
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
        assert_eq!(app.ui.composer.buffer, "/invite ");
        assert_eq!(app.ui.composer.cursor, app.ui.composer.buffer.len());
    }

    #[tokio::test]
    async fn exact_dm_autocomplete_enter_submits_command() {
        let mut app = test_app("dm-enter-submit").await;
        app.snapshot.users.push(crate::service::UserPresence {
            username: "alice".to_string(),
            display_name: "Alice".to_string(),
            last_seen_at: None,
            connected: true,
        });
        app.ui.mode = UiMode::Compose;
        app.ui.composer = ComposerState::from("/dm @alice");
        app.update_completions();

        assert!(app.ui.composer.autocomplete.open);

        app.handle_input(b"\r");

        assert_eq!(app.ui.mode, UiMode::Normal);
        assert_eq!(
            app.actions,
            vec![Action::OpenDm {
                target: "@alice".to_string()
            }]
        );
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
        assert_eq!(app.ui.prompt.prefix, "/thread new ");

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
