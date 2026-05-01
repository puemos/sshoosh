use std::{
    ops::Range,
    time::{Duration, Instant},
};

use crate::service::Snapshot;
use ratatui::layout::{Position, Rect};
use tui_scrollview::ScrollViewState;

use super::{
    action::SourceTarget,
    commands::{CommandExecutor, PaletteItem},
    input::MouseModifiers,
};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum UiMode {
    Normal,
    Compose,
    Palette,
    Help,
    ConfirmQuit,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ActivePane {
    Rail,
    List,
    Detail,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub enum Route {
    Channel(String),
    #[default]
    Dms,
    Search,
    Saved,
}

#[derive(Clone, Debug)]
pub struct UiState {
    pub mode: UiMode,
    pub active_pane: ActivePane,
    pub route: Route,
    pub threads_collapsed: bool,
    pub workspace_scroll: ScrollViewState,
    pub detail_scroll: ScrollViewState,
    pub help_scroll: ScrollViewState,
    pub composer: ComposerState,
    pub palette: PaletteState,
    pub banner: Option<Banner>,
    pub startup_splash_until: Option<Instant>,
    pub comment_menu: Option<CommentMenuState>,
    pub comment_delete: Option<CommentDeleteState>,
    pub search_selected: usize,
    pub saved_selected: usize,
    pub hit_map: HitMap,
    pub link_overlays: Vec<LinkOverlay>,
    pub message_selection_regions: Vec<MessageSelectionRegion>,
    pub selection: SelectionState,
}

impl Default for UiState {
    fn default() -> Self {
        Self {
            mode: UiMode::Normal,
            active_pane: ActivePane::List,
            route: Route::Dms,
            threads_collapsed: false,
            workspace_scroll: ScrollViewState::default(),
            detail_scroll: ScrollViewState::default(),
            help_scroll: ScrollViewState::default(),
            composer: ComposerState::default(),
            palette: PaletteState::default(),
            banner: None,
            startup_splash_until: None,
            comment_menu: None,
            comment_delete: None,
            search_selected: 0,
            saved_selected: 0,
            hit_map: HitMap::default(),
            link_overlays: Vec::new(),
            message_selection_regions: Vec::new(),
            selection: SelectionState::default(),
        }
    }
}

impl UiState {
    pub fn show_startup_splash(&mut self, duration: Duration) {
        self.startup_splash_until = Some(Instant::now() + duration);
    }

    pub fn dismiss_startup_splash(&mut self) {
        self.startup_splash_until = None;
    }

    pub fn startup_splash_active(&mut self) -> bool {
        let active = self
            .startup_splash_until
            .is_some_and(|until| Instant::now() < until);
        if !active {
            self.startup_splash_until = None;
        }
        active
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LinkOverlay {
    pub rect: Rect,
    pub url: String,
    pub text: String,
    pub style: ratatui::style::Style,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct SelectionState {
    pub pending: Option<SelectionAnchor>,
    pub range: Option<SelectionRange>,
    pub message_region: Option<MessageSelectionRegion>,
    pub text: String,
    pub copy_requested: bool,
}

impl SelectionState {
    pub fn clear(&mut self) {
        self.pending = None;
        self.range = None;
        self.message_region = None;
        self.text.clear();
        self.copy_requested = false;
    }

    pub fn active_range(&self) -> Option<SelectionRange> {
        self.range
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SelectionAnchor {
    pub at: Position,
    pub region: Option<HitRegion>,
    pub message_region: Option<MessageSelectionRegion>,
    pub modifiers: MouseModifiers,
    pub moved: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SelectionRange {
    pub start: Position,
    pub end: Position,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct MessageSelectionRegion {
    pub rect: Rect,
}

impl MessageSelectionRegion {
    pub fn contains(self, column: u16, row: u16) -> bool {
        contains(self.rect, column, row)
    }
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct HitMap {
    entries: Vec<HitRegion>,
}

impl HitMap {
    pub fn clear(&mut self) {
        self.entries.clear();
    }

    pub fn push(&mut self, rect: Rect, target: HitTarget) {
        if rect.width == 0 || rect.height == 0 {
            return;
        }
        self.entries.push(HitRegion { rect, target });
    }

    pub fn hit(&self, column: u16, row: u16) -> Option<HitRegion> {
        self.entries
            .iter()
            .rev()
            .find(|entry| contains(entry.rect, column, row))
            .cloned()
    }

    pub fn hit_matching(
        &self,
        column: u16,
        row: u16,
        predicate: impl Fn(&HitTarget) -> bool,
    ) -> Option<HitRegion> {
        self.entries
            .iter()
            .rev()
            .find(|entry| contains(entry.rect, column, row) && predicate(&entry.target))
            .cloned()
    }

    pub fn hit_row_matching(
        &self,
        row: u16,
        predicate: impl Fn(&HitTarget) -> bool,
    ) -> Option<HitRegion> {
        self.entries
            .iter()
            .rev()
            .find(|entry| {
                entry.rect.y <= row
                    && row < entry.rect.y.saturating_add(entry.rect.height)
                    && predicate(&entry.target)
            })
            .cloned()
    }

    #[cfg(test)]
    pub fn entries(&self) -> &[HitRegion] {
        &self.entries
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct HitRegion {
    pub rect: Rect,
    pub target: HitTarget,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EditableMessageTarget {
    Comment(i64),
    Dm(i64),
}

impl EditableMessageTarget {
    pub fn index(self) -> i64 {
        match self {
            Self::Comment(index) | Self::Dm(index) => index,
        }
    }

    pub fn noun(self) -> &'static str {
        match self {
            Self::Comment(_) => "comment",
            Self::Dm(_) => "message",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ReactionTarget {
    ThreadRoot,
    Comment(i64),
    Dm(i64),
}

impl ReactionTarget {
    pub fn index(self) -> Option<i64> {
        match self {
            Self::ThreadRoot => None,
            Self::Comment(index) | Self::Dm(index) => Some(index),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum HitTarget {
    WorkspaceScroll,
    WorkspaceChannel(String),
    WorkspaceThread(String),
    WorkspaceSaved,
    WorkspaceDm {
        conversation_id: Option<String>,
        username: String,
    },
    TopbarNotifications,
    TopbarMentions,
    DetailScroll,
    SearchResult(usize),
    SavedResult(usize),
    EditableMessage(EditableMessageTarget),
    ReactionChip {
        target: ReactionTarget,
        emoji: String,
        reacted_by_me: bool,
    },
    ReactionAdd {
        target: ReactionTarget,
    },
    MessageLink(String),
    ComposerInput {
        scroll_y: u16,
    },
    AutocompleteScroll,
    AutocompleteRow(usize),
    HelpScroll,
    PaletteBackdrop,
    PaletteInput,
    PaletteResults,
    PaletteRow(usize),
    HelpBackdrop,
    BannerModal,
    ListModalRow(usize),
    ConfirmQuitBackdrop,
    ConfirmQuitYes,
    ConfirmQuitNo,
    BottomBar(BottomBarAction),
    CommentMenuBackdrop,
    CommentMenuEdit(EditableMessageTarget),
    CommentMenuDelete(EditableMessageTarget),
    CommentMenuSave {
        target: EditableMessageTarget,
        saved: bool,
    },
    CommentDeleteConfirm(EditableMessageTarget),
    CommentDeleteCancel,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BottomBarAction {
    ToggleDetail,
    OpenCommand,
    OpenHelp,
    OpenQuit,
    SubmitComposer,
    AcceptAutocomplete,
    CloseMode,
    RunPalette,
    ConfirmQuit,
    CancelQuit,
}

pub(crate) fn contains(rect: Rect, column: u16, row: u16) -> bool {
    column >= rect.x
        && row >= rect.y
        && column < rect.x.saturating_add(rect.width)
        && row < rect.y.saturating_add(rect.height)
}

impl UiState {
    pub fn sync_route_from_snapshot(&mut self, snapshot: &Snapshot) {
        if let Some(conversation_id) = snapshot.selected_conversation_id.clone() {
            self.route = Route::Dms;
            if matches!(self.active_pane, ActivePane::List) {
                self.active_pane = ActivePane::Detail;
            }
            let _ = conversation_id;
        } else if self.route != Route::Search
            && self.route != Route::Saved
            && let Some(channel_id) = snapshot.selected_channel_id.clone()
        {
            self.route = Route::Channel(channel_id);
        }
    }
}

#[derive(Clone, Debug, Default)]
pub struct ComposerState {
    pub buffer: String,
    pub cursor: usize,
    pub history: Vec<String>,
    history_position: Option<usize>,
    history_draft: Option<String>,
    pub autocomplete: AutocompleteState,
    pub inline_prompt: Option<ComposerInlinePrompt>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ComposerInlinePrompt {
    pub prefix_len: usize,
    pub placeholder: String,
}

impl From<&str> for ComposerState {
    fn from(value: &str) -> Self {
        Self {
            buffer: value.to_string(),
            cursor: value.len(),
            ..Self::default()
        }
    }
}

impl ComposerState {
    pub fn start(&mut self, value: &str) {
        self.buffer = value.to_string();
        self.cursor = value.len();
        self.history_position = None;
        self.history_draft = None;
        self.autocomplete = AutocompleteState::default();
        self.inline_prompt = None;
    }

    pub fn start_prompt(&mut self, prefix: &str, placeholder: &str) {
        self.start(prefix);
        if !placeholder.is_empty() {
            self.inline_prompt = Some(ComposerInlinePrompt {
                prefix_len: self.buffer.len(),
                placeholder: placeholder.to_string(),
            });
        }
    }

    pub fn reset_input(&mut self) {
        self.start("");
    }

    pub fn insert(&mut self, ch: char) {
        self.cancel_history_navigation();
        self.buffer.insert(self.cursor, ch);
        self.cursor += ch.len_utf8();
    }

    pub fn insert_str(&mut self, text: &str) {
        self.cancel_history_navigation();
        self.buffer.insert_str(self.cursor, text);
        self.cursor += text.len();
    }

    pub fn replace_range(&mut self, range: Range<usize>, text: &str) {
        self.cancel_history_navigation();
        self.buffer.replace_range(range.clone(), text);
        self.cursor = range.start + text.len();
    }

    pub fn backspace(&mut self) {
        if self.cursor == 0 {
            return;
        }
        self.cancel_history_navigation();
        let prev = self.buffer[..self.cursor]
            .char_indices()
            .last()
            .map(|(idx, _)| idx)
            .unwrap_or(0);
        self.buffer.drain(prev..self.cursor);
        self.cursor = prev;
    }

    pub fn delete(&mut self) {
        if self.cursor >= self.buffer.len() {
            return;
        }
        self.cancel_history_navigation();
        let next = self.buffer[self.cursor..]
            .char_indices()
            .nth(1)
            .map(|(idx, _)| self.cursor + idx)
            .unwrap_or(self.buffer.len());
        self.buffer.drain(self.cursor..next);
    }

    pub fn move_left(&mut self) {
        if self.cursor == 0 {
            return;
        }
        self.cursor = self.buffer[..self.cursor]
            .char_indices()
            .last()
            .map(|(idx, _)| idx)
            .unwrap_or(0);
    }

    pub fn move_right(&mut self) {
        if self.cursor >= self.buffer.len() {
            return;
        }
        self.cursor = self.buffer[self.cursor..]
            .char_indices()
            .nth(1)
            .map(|(idx, _)| self.cursor + idx)
            .unwrap_or(self.buffer.len());
    }

    pub fn move_word_left(&mut self) {
        while self.cursor > 0 && self.buffer.as_bytes()[self.cursor - 1].is_ascii_whitespace() {
            self.move_left();
        }
        while self.cursor > 0 && !self.buffer.as_bytes()[self.cursor - 1].is_ascii_whitespace() {
            self.move_left();
        }
    }

    pub fn move_word_right(&mut self) {
        while self.cursor < self.buffer.len()
            && !self.buffer.as_bytes()[self.cursor].is_ascii_whitespace()
        {
            self.move_right();
        }
        while self.cursor < self.buffer.len()
            && self.buffer.as_bytes()[self.cursor].is_ascii_whitespace()
        {
            self.move_right();
        }
    }

    pub fn clear_before_cursor(&mut self) {
        self.cancel_history_navigation();
        self.buffer.drain(..self.cursor);
        self.cursor = 0;
    }

    pub fn clear_after_cursor(&mut self) {
        self.cancel_history_navigation();
        self.buffer.truncate(self.cursor);
    }

    pub fn delete_word_before_cursor(&mut self) {
        let end = self.cursor;
        self.move_word_left();
        self.cancel_history_navigation();
        self.buffer.drain(self.cursor..end);
    }

    pub fn push_history(&mut self, line: String) {
        if line.starts_with('/') && !line.trim().is_empty() {
            self.history.push(line);
        }
        self.history_position = None;
        self.history_draft = None;
    }

    pub fn previous_history(&mut self) -> bool {
        if self.history.is_empty() {
            return false;
        }
        let position = match self.history_position {
            Some(position) => position.saturating_sub(1),
            None => {
                self.history_draft = Some(self.buffer.clone());
                self.history.len() - 1
            }
        };
        self.load_history(position);
        true
    }

    pub fn next_history(&mut self) -> bool {
        let Some(position) = self.history_position else {
            return false;
        };
        if position + 1 < self.history.len() {
            self.load_history(position + 1);
        } else {
            let draft = self.history_draft.take().unwrap_or_default();
            self.buffer = draft;
            self.cursor = self.buffer.len();
            self.history_position = None;
        }
        true
    }

    fn load_history(&mut self, position: usize) {
        self.history_position = Some(position);
        self.buffer = self.history[position].clone();
        self.cursor = self.buffer.len();
        self.autocomplete = AutocompleteState::default();
    }

    fn cancel_history_navigation(&mut self) {
        self.history_position = None;
        self.history_draft = None;
    }
}

#[derive(Clone, Debug, Default)]
pub struct AutocompleteState {
    pub open: bool,
    pub items: Vec<AutocompleteItem>,
    pub selected: usize,
}

impl AutocompleteState {
    pub fn next(&mut self) {
        if !self.items.is_empty() {
            self.selected = (self.selected + 1) % self.items.len();
        }
    }

    pub fn previous(&mut self) {
        if !self.items.is_empty() {
            self.selected = if self.selected == 0 {
                self.items.len() - 1
            } else {
                self.selected - 1
            };
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AutocompleteItem {
    pub replacement_range: Range<usize>,
    pub replacement: String,
    pub label: String,
    pub detail: String,
    pub preview: String,
    pub accept_on_enter: bool,
    pub accept_on_tab: bool,
    pub executor: Option<CommandExecutor>,
}

#[derive(Clone, Debug, Default)]
pub struct PaletteState {
    pub query: String,
    pub items: Vec<PaletteItem>,
    pub filtered: Vec<usize>,
    pub selected: usize,
}

impl PaletteState {
    pub fn apply_filter(&mut self, query: &str) {
        let mut scored: Vec<(usize, i64)> = self
            .items
            .iter()
            .enumerate()
            .filter_map(|(idx, item)| {
                fuzzy_score(&item.search_text(), query).map(|score| (idx, score))
            })
            .collect();
        scored.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.cmp(&b.0)));
        self.filtered = scored.into_iter().map(|(idx, _)| idx).collect();
        self.selected = self.selected.min(self.filtered.len().saturating_sub(1));
    }

    pub fn next(&mut self) {
        if !self.filtered.is_empty() {
            self.selected = (self.selected + 1) % self.filtered.len();
        }
    }

    pub fn previous(&mut self) {
        if !self.filtered.is_empty() {
            self.selected = if self.selected == 0 {
                self.filtered.len() - 1
            } else {
                self.selected - 1
            };
        }
    }

    pub fn selected_item(&self) -> Option<&PaletteItem> {
        let idx = *self.filtered.get(self.selected)?;
        self.items.get(idx)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CommentMenuState {
    pub target: EditableMessageTarget,
    pub can_edit_delete: bool,
    pub saved: bool,
    pub x: u16,
    pub y: u16,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CommentDeleteState {
    pub target: EditableMessageTarget,
}

#[derive(Clone, Debug)]
pub struct Banner {
    pub text: String,
    pub error: bool,
    pub presentation: BannerPresentation,
    pub list: Option<ListModal>,
    at: Instant,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ListModal {
    pub title: String,
    pub columns: Vec<String>,
    pub rows: Vec<Vec<String>>,
    pub row_actions: Vec<Option<ListModalAction>>,
    pub empty: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ListModalAction {
    OpenSource(SourceTarget),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BannerPresentation {
    Toast,
    Modal,
    ListModal,
}

impl Banner {
    pub fn ok(text: impl Into<String>) -> Self {
        Self {
            text: text.into(),
            error: false,
            presentation: BannerPresentation::Toast,
            list: None,
            at: Instant::now(),
        }
    }

    pub fn err(text: impl Into<String>) -> Self {
        Self {
            text: text.into(),
            error: true,
            presentation: BannerPresentation::Toast,
            list: None,
            at: Instant::now(),
        }
    }

    pub fn modal_ok(text: impl Into<String>) -> Self {
        Self {
            text: text.into(),
            error: false,
            presentation: BannerPresentation::Modal,
            list: None,
            at: Instant::now(),
        }
    }

    pub fn list(list: ListModal) -> Self {
        Self {
            text: list.title.clone(),
            error: false,
            presentation: BannerPresentation::ListModal,
            list: Some(list),
            at: Instant::now(),
        }
    }

    pub fn active(&self) -> bool {
        let ttl = match self.presentation {
            BannerPresentation::Toast => Duration::from_secs(8),
            BannerPresentation::Modal | BannerPresentation::ListModal => Duration::from_secs(60),
        };
        self.at.elapsed() < ttl
    }

    pub fn modal_active(&self) -> bool {
        matches!(
            self.presentation,
            BannerPresentation::Modal | BannerPresentation::ListModal
        ) && self.active()
    }
}

pub fn fuzzy_score(haystack: &str, needle: &str) -> Option<i64> {
    if needle.trim().is_empty() {
        return Some(0);
    }
    let haystack = haystack.to_lowercase();
    let needle = needle.to_lowercase();
    if haystack.contains(&needle) {
        return Some(1000 - haystack.find(&needle).unwrap_or(0) as i64);
    }
    let mut score = 0;
    let mut pos = 0;
    for ch in needle.chars() {
        let found = haystack[pos..].find(ch)?;
        score += 20 - found as i64;
        pos += found + ch.len_utf8();
    }
    Some(score)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn composer_readline_editing() {
        let mut composer = ComposerState::from("hello world");
        composer.cursor = composer.buffer.len();
        composer.delete_word_before_cursor();
        assert_eq!(composer.buffer, "hello ");
        composer.insert_str("there");
        composer.move_word_left();
        composer.clear_after_cursor();
        assert_eq!(composer.buffer, "hello ");
    }

    #[test]
    fn fuzzy_match_handles_subsequence() {
        assert!(fuzzy_score("Create private channel", "cpc").is_some());
        assert!(fuzzy_score("Create private channel", "zzz").is_none());
    }
}
