use pulldown_cmark::{Event, Options, Parser, Tag, TagEnd};
use ratatui::{
    Frame,
    layout::{Alignment, Constraint, Direction, Layout, Position, Rect, Size},
    style::{Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Clear, List, ListItem, Padding, Paragraph, Wrap},
};
use time::{OffsetDateTime, macros::format_description};
use tui_scrollview::{ScrollView, ScrollViewState, ScrollbarVisibility};

use crate::service::{Account, SearchKind, Snapshot};

use super::{
    commands::CommandSpec,
    state::{
        ActivePane, BannerPresentation, BottomBarAction, HitTarget, LinkOverlay, Route,
        SelectionRange, UiMode, UiState,
    },
    theme,
};

const WORKSPACE_PANE_WIDTH: u16 = 38;

include!("render/shell.rs");
include!("render/selection.rs");
include!("render/chrome.rs");
include!("render/workspace.rs");
include!("render/detail.rs");
include!("render/messages.rs");
include!("render/markdown.rs");
include!("render/composer.rs");
include!("render/overlays.rs");
include!("render/tests.rs");
