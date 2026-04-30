use std::{
    ops::Range,
    time::{Duration, Instant},
};

use crate::service::Snapshot;
use ratatui::layout::{Position, Rect};
use tui_scrollview::ScrollViewState;

use super::{commands::PaletteItem, input::MouseModifiers};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum UiMode {
    Normal,
    Compose,
    Palette,
    Prompt,
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
}

#[derive(Clone, Debug)]
pub struct UiState {
    pub mode: UiMode,
    pub active_pane: ActivePane,
    pub route: Route,
    pub threads_collapsed: bool,
    pub workspace_scroll: ScrollViewState,
    pub detail_scroll: ScrollViewState,
    pub composer: ComposerState,
    pub palette: PaletteState,
    pub prompt: PromptState,
    pub banner: Option<Banner>,
    pub search_selected: usize,
    pub hit_map: HitMap,
    pub link_overlays: Vec<LinkOverlay>,
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
            composer: ComposerState::default(),
            palette: PaletteState::default(),
            prompt: PromptState::default(),
            banner: None,
            search_selected: 0,
            hit_map: HitMap::default(),
            link_overlays: Vec::new(),
            selection: SelectionState::default(),
        }
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
    pub text: String,
    pub copy_requested: bool,
}

impl SelectionState {
    pub fn clear(&mut self) {
        self.pending = None;
        self.range = None;
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
    pub modifiers: MouseModifiers,
    pub moved: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SelectionRange {
    pub start: Position,
    pub end: Position,
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

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum HitTarget {
    WorkspaceScroll,
    WorkspaceChannel(String),
    WorkspaceThread(String),
    WorkspaceDm(String),
    DetailScroll,
    MessageLink(String),
    ComposerInput { scroll_y: u16 },
    AutocompleteScroll,
    AutocompleteRow(usize),
    PaletteBackdrop,
    PaletteInput,
    PaletteResults,
    PaletteRow(usize),
    PromptBackdrop,
    PromptInput,
    HelpBackdrop,
    BannerModal,
    ConfirmQuitBackdrop,
    ConfirmQuitYes,
    ConfirmQuitNo,
    BottomBar(BottomBarAction),
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
    RunPrompt,
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
    pub autocomplete: AutocompleteState,
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
    pub fn insert(&mut self, ch: char) {
        self.buffer.insert(self.cursor, ch);
        self.cursor += ch.len_utf8();
    }

    pub fn insert_str(&mut self, text: &str) {
        self.buffer.insert_str(self.cursor, text);
        self.cursor += text.len();
    }

    pub fn replace_range(&mut self, range: Range<usize>, text: &str) {
        self.buffer.replace_range(range.clone(), text);
        self.cursor = range.start + text.len();
    }

    pub fn backspace(&mut self) {
        if self.cursor == 0 {
            return;
        }
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
        self.buffer.drain(..self.cursor);
        self.cursor = 0;
    }

    pub fn clear_after_cursor(&mut self) {
        self.buffer.truncate(self.cursor);
    }

    pub fn delete_word_before_cursor(&mut self) {
        let end = self.cursor;
        self.move_word_left();
        self.buffer.drain(self.cursor..end);
    }

    pub fn push_history(&mut self, line: String) {
        if !line.trim().is_empty() {
            self.history.push(line);
        }
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

    pub fn selected_replacement(&self) -> Option<(Range<usize>, String)> {
        if !self.open {
            return None;
        }
        let item = self.items.get(self.selected)?;
        if item.accept_on_enter {
            Some((item.replacement_range.clone(), item.replacement.clone()))
        } else {
            None
        }
    }

    pub fn selected_tab_replacement(&self) -> Option<(Range<usize>, String)> {
        if !self.open {
            return None;
        }
        let item = self.items.get(self.selected)?;
        if item.accept_on_tab {
            Some((item.replacement_range.clone(), item.replacement.clone()))
        } else {
            None
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

#[derive(Clone, Debug, Default)]
pub struct PromptState {
    pub title: String,
    pub prefix: String,
    pub placeholder: String,
    pub input: String,
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
    pub empty: String,
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
