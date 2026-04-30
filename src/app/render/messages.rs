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

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum HeaderMode {
    Full,
    Suppressed,
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn message_card<'a>(
    snapshot: &Snapshot,
    kind: MessageKind,
    header_mode: HeaderMode,
    avoid_color: Option<Color>,
    author: &str,
    created_at: Option<&str>,
    edited_at: Option<&str>,
    reactions: Option<&str>,
    body: &str,
    width: usize,
) -> MessageCard<'a> {
    let _ = snapshot.current_username.as_deref();
    let surface = message_surface(body);
    let author_color = theme::author_color_avoiding(author, avoid_color);
    let gutter_color = if is_error_message(body) {
        theme::MESSAGE_ERROR_GUTTER
    } else {
        author_color
    };
    let gutter = theme::message_gutter(gutter_color, surface);
    let valid_mentions: Vec<String> = snapshot
        .users
        .iter()
        .map(|user| user.username.to_ascii_lowercase())
        .collect();
    let mut lines = Vec::new();
    let mut links = Vec::new();
    let mut row_idx: usize = 0;

    if matches!(header_mode, HeaderMode::Full) {
        lines.push(message_card_line(
            gutter,
            header_spans(
                kind,
                author,
                author_color,
                created_at,
                reactions,
                surface,
                width,
            ),
        ));
        row_idx += 1;
    }

    let body_rows: Vec<_> = render_message_body_with_mentions(body, width, &valid_mentions)
        .into_iter()
        .collect();
    let last_body_idx = body_rows.len().saturating_sub(1);
    for (idx, row) in body_rows.into_iter().enumerate() {
        let mut col = MESSAGE_PREFIX_WIDTH;
        let mut content = Vec::new();
        let mut last_visible_chars: usize = 0;
        for run in row {
            let style = run.style.bg(surface);
            let chars = run.text.chars().count();
            let span_width = chars.min(u16::MAX as usize) as u16;
            if let Some(url) = run
                .link_url
                .as_ref()
                .filter(|url| is_openable_link_url(url))
                && span_width > 0
            {
                links.push(MessageLinkHit {
                    row: row_idx.min(u16::MAX as usize) as u16,
                    col,
                    width: span_width,
                    url: url.clone(),
                    text: run.text.clone(),
                    style,
                });
            }
            col = col.saturating_add(span_width);
            last_visible_chars = last_visible_chars.saturating_add(chars);
            content.push(Span::styled(run.text, style));
        }
        // Append "(edited)" inline at end of the last body line.
        if idx == last_body_idx
            && edited_at.is_some()
            && last_visible_chars + 1 + EDITED_TAG.chars().count() <= width
        {
            content.push(Span::styled(
                format!(" {EDITED_TAG}"),
                theme::message_meta_on(surface),
            ));
        }
        lines.push(message_card_line(gutter, content));
        row_idx += 1;
    }

    MessageCard {
        item: ListItem::new(lines).style(theme::message_card_on(surface)),
        links,
        hit: None,
    }
}

const EDITED_TAG: &str = "(edited)";

#[allow(clippy::too_many_arguments)]
fn header_spans<'a>(
    kind: MessageKind,
    author: &str,
    author_color: Color,
    created_at: Option<&str>,
    reactions: Option<&str>,
    surface: Color,
    width: usize,
) -> Vec<Span<'a>> {
    let author_text = format!("@{}", sanitize_terminal_visible_text(author));
    let author_chars = author_text.chars().count();

    let mut right_parts: Vec<String> = Vec::new();
    if let Some(created_at) = created_at {
        right_parts.push(format_human_timestamp(created_at));
    }
    if matches!(kind, MessageKind::ThreadRoot) {
        right_parts.push("thread root".to_string());
    }
    if let Some(reactions) = reactions.filter(|value| !value.is_empty()) {
        right_parts.push(sanitize_terminal_visible_text(reactions));
    }
    let right = right_parts.join(" · ");
    let right_chars = right.chars().count();

    let mut spans = vec![Span::styled(
        author_text,
        theme::message_author_on(author_color, surface),
    )];
    let used = author_chars.saturating_add(right_chars);
    if width > used && !right.is_empty() {
        let pad = width - used;
        spans.push(Span::styled(
            " ".repeat(pad),
            theme::message_meta_on(surface),
        ));
        spans.push(Span::styled(right, theme::message_meta_on(surface)));
    } else if !right.is_empty() {
        // Not enough room to right-align — fall back to inline.
        spans.push(Span::styled(
            format!(" · {right}"),
            theme::message_meta_on(surface),
        ));
    }
    spans
}

fn message_surface(body: &str) -> Color {
    if is_error_message(body) {
        theme::MESSAGE_CARD_FOCUSED
    } else {
        theme::MESSAGE_CARD
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

pub(crate) fn date_divider<'a>(label: &str, width: usize) -> ListItem<'a> {
    let label_text = format!(" {label} ");
    let label_width = label_text.chars().count();
    let total = width.max(label_width + 4);
    let side = (total - label_width) / 2;
    let left = "─".repeat(side);
    let right = "─".repeat(total - side - label_width);
    ListItem::new(Line::from(vec![
        Span::styled(left, theme::message_separator()),
        Span::styled(label_text, theme::muted()),
        Span::styled(right, theme::message_separator()),
    ]))
    .style(theme::panel())
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
