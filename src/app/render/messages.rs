use super::*;
use crate::app::state::MessageSelectionRegion;
use crate::service::ReactionSummary;
use crate::time_format::format_human_timestamp;
use ratatui::style::Color;

const MESSAGE_PREFIX: &str = "";
const MESSAGE_PREFIX_WIDTH: u16 = 0;
pub(crate) const SAVED_MARKER: &str = "◆";

pub(crate) struct MessageCard<'a> {
    item: ListItem<'a>,
    links: Vec<MessageLinkHit>,
    mentions: Vec<MessageMentionHit>,
    labels: Vec<MessageLabelHit>,
    reactions: Vec<MessageReactionHit>,
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

pub(crate) struct MessageMentionHit {
    row: u16,
    col: u16,
    width: u16,
    username: String,
}

pub(crate) struct MessageLabelHit {
    row: u16,
    col: u16,
    width: u16,
    tag: String,
}

pub(crate) struct MessageCardHit {
    row: u16,
    height: u16,
    target: HitTarget,
}

pub(crate) struct MessageReactionHit {
    row: u16,
    col: u16,
    width: u16,
    target: HitTarget,
}

pub(crate) struct MessageSelectionHit {
    row: u16,
    height: u16,
}

#[derive(Default)]
pub(crate) struct MessageHits {
    links: Vec<MessageLinkHit>,
    mentions: Vec<MessageMentionHit>,
    labels: Vec<MessageLabelHit>,
    reactions: Vec<MessageReactionHit>,
    cards: Vec<MessageCardHit>,
    selections: Vec<MessageSelectionHit>,
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

pub(crate) fn resolve_author_color(snapshot: &Snapshot, author: &str) -> Color {
    let lower = author.to_ascii_lowercase();
    if let Some(index) = snapshot
        .users
        .iter()
        .position(|user| user.username.eq_ignore_ascii_case(&lower))
    {
        theme::author_color_for_index(index)
    } else {
        theme::author_color_fallback(author)
    }
}

pub(crate) struct MessageCardSpec<'a> {
    pub(crate) snapshot: &'a Snapshot,
    pub(crate) kind: MessageKind,
    pub(crate) header_mode: HeaderMode,
    pub(crate) author: &'a str,
    pub(crate) created_at: Option<&'a str>,
    pub(crate) edited_at: Option<&'a str>,
    pub(crate) saved: bool,
    pub(crate) reactions: &'a [ReactionSummary],
    pub(crate) reaction_target: Option<ReactionTarget>,
    pub(crate) body: &'a str,
    pub(crate) width: usize,
    /// Optional pre-styled spans rendered as a leading row above the header.
    /// Feed views (saved/labels/notifications) use this for source breadcrumbs.
    pub(crate) breadcrumb: Option<Vec<Span<'static>>>,
    /// When true, paint the card with the elevated surface to indicate selection.
    pub(crate) selected: bool,
}

pub(crate) fn message_card(spec: MessageCardSpec<'_>) -> MessageCard<'static> {
    let MessageCardSpec {
        snapshot,
        kind,
        header_mode,
        author,
        created_at,
        edited_at,
        saved,
        reactions,
        reaction_target,
        body,
        width,
        breadcrumb,
        selected,
    } = spec;
    let current_username = snapshot.current_username.as_deref();
    let surface = message_surface(body, selected);
    let author_color = resolve_author_color(snapshot, author);
    let gutter = theme::message_card_on(surface);
    let valid_mentions: Vec<String> = snapshot
        .users
        .iter()
        .map(|user| user.username.clone())
        .collect();
    let mut lines = Vec::new();
    let mut links = Vec::new();
    let mut mentions = Vec::new();
    let mut labels = Vec::new();
    let mut reaction_hits = Vec::new();
    let mut row_idx: usize = 0;

    if let Some(spans) = breadcrumb {
        let rebound: Vec<Span<'static>> = spans
            .into_iter()
            .map(|span| {
                let style = span.style.bg(surface);
                Span::styled(span.content, style)
            })
            .collect();
        lines.push(message_card_line(gutter, rebound));
        row_idx += 1;
    }

    if matches!(header_mode, HeaderMode::Full) {
        lines.push(message_card_line(
            gutter,
            header_spans(
                kind,
                author,
                author_color,
                created_at,
                saved,
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
            if let Some(username) = run.mention_username.as_ref()
                && span_width > 0
                && !current_username.is_some_and(|current| current.eq_ignore_ascii_case(username))
            {
                mentions.push(MessageMentionHit {
                    row: row_idx.min(u16::MAX as usize) as u16,
                    col,
                    width: span_width,
                    username: username.clone(),
                });
            }
            if let Some(tag) = run
                .link_url
                .as_ref()
                .and_then(|url| url.strip_prefix(LABEL_LINK_PREFIX))
                && span_width > 0
            {
                labels.push(MessageLabelHit {
                    row: row_idx.min(u16::MAX as usize) as u16,
                    col,
                    width: span_width,
                    tag: tag.to_string(),
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

    if let Some(target) = reaction_target {
        for row in reaction_rows(reactions, target, surface, width) {
            let hit_row = row_idx.min(u16::MAX as usize) as u16;
            for hit in row.hits {
                reaction_hits.push(MessageReactionHit {
                    row: hit_row,
                    col: hit.col,
                    width: hit.width,
                    target: hit.target,
                });
            }
            lines.push(message_card_line(gutter, row.spans));
            row_idx += 1;
        }
    }

    MessageCard {
        item: ListItem::new(lines).style(theme::message_card_on(surface)),
        links,
        mentions,
        labels,
        reactions: reaction_hits,
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
    saved: bool,
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
    let right = right_parts.join(" · ");
    let saved_prefix = if saved && !right.is_empty() {
        " · "
    } else {
        ""
    };
    let saved_width = if saved {
        SAVED_MARKER.chars().count()
    } else {
        0
    };
    let right_chars = right
        .chars()
        .count()
        .saturating_add(saved_prefix.chars().count())
        .saturating_add(saved_width);

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
        if !right.is_empty() {
            spans.push(Span::styled(right, theme::message_meta_on(surface)));
        }
        if saved {
            spans.push(Span::styled(saved_prefix, theme::message_meta_on(surface)));
            spans.push(Span::styled(SAVED_MARKER, theme::message_saved_on(surface)));
        }
    } else if !right.is_empty() || saved {
        // Not enough room to right-align — fall back to inline.
        if !right.is_empty() {
            spans.push(Span::styled(
                format!(" · {right}"),
                theme::message_meta_on(surface),
            ));
        }
        if saved {
            spans.push(Span::styled(saved_prefix, theme::message_meta_on(surface)));
            spans.push(Span::styled(SAVED_MARKER, theme::message_saved_on(surface)));
        }
    }
    spans
}

struct ReactionRow<'a> {
    spans: Vec<Span<'a>>,
    hits: Vec<ReactionRowHit>,
}

struct ReactionRowHit {
    col: u16,
    width: u16,
    target: HitTarget,
}

fn reaction_rows<'a>(
    reactions: &[ReactionSummary],
    target: ReactionTarget,
    surface: Color,
    width: usize,
) -> Vec<ReactionRow<'a>> {
    let mut rows = Vec::new();
    let mut spans = Vec::new();
    let mut hits = Vec::new();
    let mut used = 0usize;
    let gap = 1usize;

    for chip in reaction_chips(reactions, target) {
        let chip_width = chip.label.chars().count();
        if used > 0 && used + gap + chip_width > width {
            rows.push(ReactionRow { spans, hits });
            spans = Vec::new();
            hits = Vec::new();
            used = 0;
        }
        if used > 0 {
            spans.push(Span::styled(" ", theme::message_card_on(surface)));
            used += gap;
        }
        hits.push(ReactionRowHit {
            col: used.min(u16::MAX as usize) as u16,
            width: chip_width.min(u16::MAX as usize) as u16,
            target: chip.target,
        });
        spans.push(Span::styled(chip.label, chip.style));
        used = used.saturating_add(chip_width);
    }

    if !spans.is_empty() {
        rows.push(ReactionRow { spans, hits });
    }
    rows
}

struct ReactionChip {
    label: String,
    style: Style,
    target: HitTarget,
}

fn reaction_chips(reactions: &[ReactionSummary], target: ReactionTarget) -> Vec<ReactionChip> {
    let mut chips = reactions
        .iter()
        .map(|reaction| ReactionChip {
            label: format!(
                " {} {} ",
                sanitize_terminal_visible_text(&reaction.emoji),
                reaction.count
            ),
            style: theme::reaction_chip(reaction.reacted_by_me),
            target: HitTarget::ReactionChip {
                target,
                emoji: reaction.emoji.clone(),
                reacted_by_me: reaction.reacted_by_me,
            },
        })
        .collect::<Vec<_>>();
    chips.push(ReactionChip {
        label: " + ".to_string(),
        style: theme::reaction_add_chip(),
        target: HitTarget::ReactionAdd { target },
    });
    chips
}

fn message_surface(_body: &str, selected: bool) -> Color {
    if selected {
        theme::ELEVATED_PANEL
    } else {
        theme::PANEL
    }
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
    hits: &mut MessageHits,
    content_row: &mut u16,
    card: MessageCard<'a>,
) {
    let height = card.item.height().min(u16::MAX as usize) as u16;
    for mut link in card.links {
        link.row = link.row.saturating_add(*content_row);
        hits.links.push(link);
    }
    for mut mention in card.mentions {
        mention.row = mention.row.saturating_add(*content_row);
        hits.mentions.push(mention);
    }
    for mut label in card.labels {
        label.row = label.row.saturating_add(*content_row);
        hits.labels.push(label);
    }
    for mut reaction in card.reactions {
        reaction.row = reaction.row.saturating_add(*content_row);
        hits.reactions.push(reaction);
    }
    if let Some(mut hit) = card.hit {
        hit.row = hit.row.saturating_add(*content_row);
        hits.cards.push(hit);
    }
    hits.selections.push(MessageSelectionHit {
        row: *content_row,
        height,
    });
    *content_row = content_row.saturating_add(height);
    items.push(card.item);
}

pub(crate) fn register_message_hits(
    ui: &mut UiState,
    area: Rect,
    hits: MessageHits,
    offset_y: u16,
) {
    register_card_hits(ui, area, hits.cards, offset_y);
    register_reaction_hits(ui, area, hits.reactions, offset_y);
    register_link_hits(ui, area, hits.links, offset_y);
    register_mention_hits(ui, area, hits.mentions, offset_y);
    register_label_hits(ui, area, hits.labels, offset_y);
    register_message_selection_regions(ui, area, hits.selections, offset_y);
}

pub(crate) fn register_mention_hits(
    ui: &mut UiState,
    area: Rect,
    mention_hits: Vec<MessageMentionHit>,
    offset_y: u16,
) {
    let bottom = offset_y.saturating_add(area.height);
    for mention in mention_hits {
        if mention.row < offset_y || mention.row >= bottom {
            continue;
        }
        let Some(x) = area.x.checked_add(mention.col) else {
            continue;
        };
        let right = area.x.saturating_add(area.width);
        if x >= right {
            continue;
        }
        let width = mention.width.min(right.saturating_sub(x));
        ui.hit_map.push(
            Rect::new(x, area.y + mention.row.saturating_sub(offset_y), width, 1),
            HitTarget::MessageMention(mention.username),
        );
    }
}

pub(crate) fn register_label_hits(
    ui: &mut UiState,
    area: Rect,
    label_hits: Vec<MessageLabelHit>,
    offset_y: u16,
) {
    let bottom = offset_y.saturating_add(area.height);
    for label in label_hits {
        if label.row < offset_y || label.row >= bottom {
            continue;
        }
        let Some(x) = area.x.checked_add(label.col) else {
            continue;
        };
        let right = area.x.saturating_add(area.width);
        if x >= right {
            continue;
        }
        let width = label.width.min(right.saturating_sub(x));
        ui.hit_map.push(
            Rect::new(x, area.y + label.row.saturating_sub(offset_y), width, 1),
            HitTarget::MessageLabel(label.tag),
        );
    }
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

pub(crate) fn register_reaction_hits(
    ui: &mut UiState,
    area: Rect,
    reaction_hits: Vec<MessageReactionHit>,
    offset_y: u16,
) {
    let bottom = offset_y.saturating_add(area.height);
    for reaction in reaction_hits {
        if reaction.row < offset_y || reaction.row >= bottom {
            continue;
        }
        let Some(x) = area.x.checked_add(reaction.col) else {
            continue;
        };
        let right = area.x.saturating_add(area.width);
        if x >= right {
            continue;
        }
        let width = reaction.width.min(right.saturating_sub(x));
        ui.hit_map.push(
            Rect::new(x, area.y + reaction.row.saturating_sub(offset_y), width, 1),
            reaction.target,
        );
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

pub(crate) fn register_message_selection_regions(
    ui: &mut UiState,
    area: Rect,
    selection_hits: Vec<MessageSelectionHit>,
    offset_y: u16,
) {
    let bottom = offset_y.saturating_add(area.height);
    for hit in selection_hits {
        let hit_bottom = hit.row.saturating_add(hit.height);
        if hit_bottom <= offset_y || hit.row >= bottom {
            continue;
        }
        let y = area.y + hit.row.saturating_sub(offset_y);
        let clipped_bottom = hit_bottom.min(bottom);
        let height = clipped_bottom.saturating_sub(offset_y.max(hit.row));
        ui.message_selection_regions.push(MessageSelectionRegion {
            rect: Rect::new(area.x, y, area.width, height),
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
    area.width.max(8) as usize
}
