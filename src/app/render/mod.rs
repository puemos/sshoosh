use pulldown_cmark::{Event, Options, Parser, Tag, TagEnd};
use ratatui::{
    Frame,
    layout::{Alignment, Constraint, Direction, Layout, Position, Rect, Size},
    style::{Modifier, Style},
    text::{Line, Span},
    widgets::{
        Block, Clear, List, ListItem, Paragraph, Scrollbar, ScrollbarOrientation, ScrollbarState,
        Wrap,
    },
};
use tui_scrollview::{ScrollView, ScrollViewState, ScrollbarVisibility};

use crate::features::{
    accounts::model::Account, feeds::model::SearchKind, messages::model::Snapshot,
    notifications::model::NotificationSummary,
};

use super::{
    commands::{CommandSpec, SubcommandSpec, subcommands_for},
    state::{
        ActivePane, Banner, BannerPresentation, BottomBarAction, DetailScrollMetrics,
        EditableMessageTarget, HitTarget, LinkOverlay, ListModal, NotificationFilter,
        ReactionTarget, Route, SelectionRange, UiMode, UiState,
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
