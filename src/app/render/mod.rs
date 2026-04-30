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
        ActivePane, Banner, BannerPresentation, BottomBarAction, HitTarget, LinkOverlay, Route,
        SelectionRange, UiMode, UiState,
    },
    theme,
};

const WORKSPACE_PANE_WIDTH: u16 = 38;

mod chrome;
mod composer;
mod detail;
mod markdown;
mod messages;
mod overlays;
mod selection;
mod shell;
mod tests;
mod workspace;

pub(crate) use self::{
    chrome::*, composer::*, detail::*, markdown::*, messages::*, overlays::*, selection::*,
    shell::*, workspace::*,
};
