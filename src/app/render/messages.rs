use super::*;
use crate::time_format::format_human_timestamp;
use ratatui::style::Color;

const MESSAGE_PREFIX: &str = "▏  ";
const MESSAGE_PREFIX_WIDTH: u16 = 3;

pub(crate) struct MessageCard<'a> {
    item: ListItem<'a>,
    links: Vec<MessageLinkHit>,
    hit: Option<MessageCardHit>,
}

#[cfg(test)]
impl MessageCard<'_> {
    pub(crate) fn height(&self) -> usize {
        self.item.height()
    }

    pub(crate) fn link_count(&self) -> usize {
        self.links.len()
    }
}

pub(crate) struct MessageLinkHit {
    row: u16,
    col: u16,
    width: u16,
    url: String,
    text: String,
    style: Style,
}

pub(crate) struct MessageCardHit {
    row: u16,
    height: u16,
    target: HitTarget,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum MessageKind {
    ThreadRoot,
    Comment,
    Dm,
}

pub(crate) fn message_card<'a>(
    snapshot: &Snapshot,
    kind: MessageKind,
    author: &str,
    created_at: Option<&str>,
    edited_at: Option<&str>,
    reactions: Option<&str>,
    body: &str,
    width: usize,
) -> MessageCard<'a> {
    let is_current_user = snapshot
        .current_username
        .as_deref()
        .is_some_and(|username| username.eq_ignore_ascii_case(author));
    let surface = message_surface(kind, body);
    let gutter = theme::message_gutter(message_gutter(kind, is_current_user, body), surface);
    let mut lines = Vec::new();
    let mut links = Vec::new();

    lines.push(message_card_line(
        gutter,
        message_meta_spans(
            kind,
            is_current_user,
            author,
            created_at,
            edited_at,
            reactions,
            surface,
        ),
    ));

    for (row_idx, row) in render_message_body(body, width).into_iter().enumerate() {
        let row_idx = row_idx.saturating_add(1);
        let mut col = MESSAGE_PREFIX_WIDTH;
        let mut content = Vec::new();
        for run in row {
            let style = run.style.bg(surface);
            let width = run.text.chars().count().min(u16::MAX as usize) as u16;
            if let Some(url) = run
                .link_url
                .as_ref()
                .filter(|url| is_openable_link_url(url))
                && width > 0
            {
                links.push(MessageLinkHit {
                    row: row_idx.min(u16::MAX as usize) as u16,
                    col,
                    width,
                    url: url.clone(),
                    text: run.text.clone(),
                    style,
                });
            }
            col = col.saturating_add(width);
            content.push(Span::styled(run.text, style));
        }
        lines.push(message_card_line(gutter, content));
    }

    MessageCard {
        item: ListItem::new(lines).style(theme::message_card_on(surface)),
        links,
        hit: None,
    }
}

fn message_meta_spans<'a>(
    kind: MessageKind,
    is_current_user: bool,
    author: &str,
    created_at: Option<&str>,
    edited_at: Option<&str>,
    reactions: Option<&str>,
    surface: Color,
) -> Vec<Span<'a>> {
    let mut meta = vec![Span::styled(
        format!("@{}", author),
        theme::message_author_on(is_current_user, surface),
    )];
    if let Some(created_at) = created_at {
        meta.push(Span::styled(
            format!(" · {}", format_human_timestamp(created_at)),
            theme::message_meta_on(surface),
        ));
    }
    if matches!(kind, MessageKind::ThreadRoot) {
        meta.push(Span::styled(
            " · thread root",
            theme::message_meta_on(surface),
        ));
    }
    if edited_at.is_some() {
        meta.push(Span::styled(" · edited", theme::message_meta_on(surface)));
    }
    if let Some(reactions) = reactions.filter(|value| !value.is_empty()) {
        meta.push(Span::styled(
            format!(" · {reactions}"),
            theme::message_meta_on(surface),
        ));
    }
    meta
}

fn message_surface(kind: MessageKind, body: &str) -> Color {
    if matches!(kind, MessageKind::ThreadRoot) {
        theme::MESSAGE_CARD_ROOT
    } else if is_error_message(body) {
        theme::MESSAGE_CARD_FOCUSED
    } else {
        theme::MESSAGE_CARD
    }
}

fn message_gutter(kind: MessageKind, is_current_user: bool, body: &str) -> Color {
    if is_error_message(body) {
        theme::MESSAGE_ERROR_GUTTER
    } else if matches!(kind, MessageKind::ThreadRoot) {
        theme::MESSAGE_ROOT_GUTTER
    } else if is_current_user {
        theme::MESSAGE_CURRENT_USER_GUTTER
    } else {
        theme::MESSAGE_GUTTER
    }
}

pub(crate) fn is_error_message(body: &str) -> bool {
    let body = body.trim_start();
    body.starts_with("Error from provider:")
        || body.starts_with("Error:")
        || body.starts_with("error:")
        || body.starts_with("Failed:")
        || body.starts_with("failed:")
}

pub(crate) fn with_message_card_hit<'a>(
    mut card: MessageCard<'a>,
    target: HitTarget,
) -> MessageCard<'a> {
    card.hit = Some(MessageCardHit {
        row: 0,
        height: card.item.height().min(u16::MAX as usize) as u16,
        target,
    });
    card
}

pub(crate) fn append_plain_item<'a>(
    items: &mut Vec<ListItem<'a>>,
    content_row: &mut u16,
    item: ListItem<'a>,
) {
    *content_row = content_row.saturating_add(item.height().min(u16::MAX as usize) as u16);
    items.push(item);
}

pub(crate) fn append_message_card<'a>(
    items: &mut Vec<ListItem<'a>>,
    link_hits: &mut Vec<MessageLinkHit>,
    card_hits: &mut Vec<MessageCardHit>,
    content_row: &mut u16,
    card: MessageCard<'a>,
) {
    for mut link in card.links {
        link.row = link.row.saturating_add(*content_row);
        link_hits.push(link);
    }
    if let Some(mut hit) = card.hit {
        hit.row = hit.row.saturating_add(*content_row);
        card_hits.push(hit);
    }
    *content_row = content_row.saturating_add(card.item.height().min(u16::MAX as usize) as u16);
    items.push(card.item);
}

pub(crate) fn register_card_hits(
    ui: &mut UiState,
    area: Rect,
    card_hits: Vec<MessageCardHit>,
    offset_y: u16,
) {
    let bottom = offset_y.saturating_add(area.height);
    for hit in card_hits {
        let hit_bottom = hit.row.saturating_add(hit.height);
        if hit_bottom <= offset_y || hit.row >= bottom {
            continue;
        }
        let y = area.y + hit.row.saturating_sub(offset_y);
        let clipped_bottom = hit_bottom.min(bottom);
        let height = clipped_bottom.saturating_sub(offset_y.max(hit.row));
        ui.hit_map
            .push(Rect::new(area.x, y, area.width, height), hit.target);
    }
}

pub(crate) fn register_link_hits(
    ui: &mut UiState,
    area: Rect,
    link_hits: Vec<MessageLinkHit>,
    offset_y: u16,
) {
    let bottom = offset_y.saturating_add(area.height);
    for link in link_hits {
        if link.row < offset_y || link.row >= bottom {
            continue;
        }
        let Some(x) = area.x.checked_add(link.col) else {
            continue;
        };
        let right = area.x.saturating_add(area.width);
        if x >= right {
            continue;
        }
        let width = link.width.min(right.saturating_sub(x));
        let rect = Rect::new(x, area.y + link.row.saturating_sub(offset_y), width, 1);
        ui.hit_map
            .push(rect, HitTarget::MessageLink(link.url.clone()));
        ui.link_overlays.push(LinkOverlay {
            rect,
            url: link.url,
            text: link.text,
            style: link.style,
        });
    }
}

pub(crate) fn message_gap<'a>() -> ListItem<'a> {
    ListItem::new(Line::from(Span::styled("", theme::message_separator()))).style(theme::panel())
}

pub(crate) fn message_card_line<'a>(gutter: Style, content: Vec<Span<'a>>) -> Line<'a> {
    let mut spans = vec![Span::styled(MESSAGE_PREFIX, gutter)];
    spans.extend(content);
    Line::from(spans)
}

pub(crate) fn message_content_width(area: Rect) -> usize {
    area.width.saturating_sub(MESSAGE_PREFIX_WIDTH + 2).max(8) as usize
}
