mod action;
mod commands;
mod input;
mod render;
mod state;
mod theme;
pub use action::{Action, SourceTarget};

use std::collections::{HashSet, VecDeque};

use ratatui::layout::{Position, Rect};

use crate::{
    client::ClientSession,
    service::{
        Account, DEFAULT_HISTORY_LIMIT, LiveEvent, MAX_HISTORY_LIMIT, NotificationSummary,
        SearchResult, ServerState, Snapshot,
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
        ActivePane, Banner, BottomBarAction, CommentDeleteState, CommentMenuState, ComposerState,
        EditableMessageTarget, HitRegion, HitTarget, PaletteState, PromptState, Route,
        SelectionAnchor, SelectionRange, UiMode, UiState,
    },
};

pub use self::state::{ListModal, ListModalAction};
pub(crate) use self::util::*;

pub struct App {
    pub running: bool,
    terminal: SshooshTerminal,
    shared: SharedBuffer,
    pub account: Account,
    client: ClientSession,
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
    seen_notification_ids: HashSet<String>,
    pending_terminal_notifications: VecDeque<TerminalNotification>,
    emitted_terminal_title: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum WorkspaceRow {
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

#[derive(Clone, Debug, PartialEq, Eq)]
struct TerminalNotification {
    id: String,
    title: String,
    body: String,
}

mod compose;
mod input_handlers;
mod lifecycle;
mod navigation;
mod render_bridge;
mod tests;
mod util;
