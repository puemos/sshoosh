mod action;
mod commands;
mod input;
mod render;
mod state;
mod theme;
pub(crate) use action::ActionDomain;
pub use action::{Action, LoadMoreRequest, SourceFocus, SourceTarget};

use std::{
    collections::{HashSet, VecDeque},
    sync::Arc,
    time::Duration,
};

use ratatui::layout::{Position, Rect};

use crate::{
    client::ClientSession,
    features::{
        accounts::model::Account,
        events::model::LiveEvent,
        feeds::model::SearchResult,
        messages::model::{
            LabelFeedItem, LabelFeedKind, SavedMessageItem, SavedMessageKind, Snapshot,
        },
        notifications::model::NotificationSummary,
        system::{DEFAULT_HISTORY_LIMIT, MAX_HISTORY_LIMIT, ServerState},
    },
    terminal::TerminalCapabilities,
    terminal::{self, SharedBuffer, SshooshTerminal},
};

const DEFAULT_SEARCH_LIMIT: i64 = 50;

use self::{
    commands::{CommandExecutor, CommandRegistry},
    input::MouseButton,
    state::{
        AccountFocus, AccountInputTarget, ActivePane, Banner, BottomBarAction, CommentDeleteState,
        CommentMenuState, ComposerState, EditableMessageTarget, HitRegion, HitTarget,
        NotificationFilter, PaletteState, ReactionTarget, Route, SelectionAnchor, SelectionRange,
        UiMode, UiState,
    },
};

pub(crate) use self::input::{InputDecoder, Key, MouseEvent, MouseEventKind};

pub use self::state::{ListModal, ListModalAction};
pub(crate) use self::util::*;

pub struct App {
    pub running: bool,
    terminal: SshooshTerminal,
    shared: SharedBuffer,
    terminal_capabilities: TerminalCapabilities,
    pub account: Account,
    client: ClientSession,
    live_rx: tokio::sync::broadcast::Receiver<LiveEvent>,
    snapshot: Snapshot,
    ui: UiState,
    commands: CommandRegistry,
    actions: Vec<Action>,
    refresh_requested: bool,
    pending_link_open: Option<String>,
    pending_clipboard_copy: Option<String>,
    desired_pointer_shape: PointerShape,
    emitted_pointer_shape: PointerShape,
    history_limit: i64,
    search_limit: i64,
    saved_limit: i64,
    label_limit: i64,
    seen_notification_ids: HashSet<String>,
    pending_terminal_notifications: VecDeque<TerminalNotification>,
    emitted_terminal_title: Option<String>,
    pub(crate) refresh_lock: Arc<tokio::sync::Mutex<()>>,
    pending_load_more: HashSet<LoadMoreRequest>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum WorkspaceRow {
    Account,
    Channel(String),
    Thread(String),
    Label(String),
    LabelsMore,
    Saved,
    Notifications,
    Dm {
        conversation_id: Option<String>,
        username: String,
    },
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
pub(crate) mod lifecycle;
mod navigation;
mod render_bridge;
mod tests;
mod util;
