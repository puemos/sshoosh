struct MessageCard<'a> {
    item: ListItem<'a>,
    links: Vec<MessageLinkHit>,
}

struct MessageLinkHit {
    row: u16,
    col: u16,
    width: u16,
    url: String,
    text: String,
    style: Style,
}

fn message_card<'a>(
    snapshot: &Snapshot,
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
    let gutter = Style::default().fg(theme::BORDER).bg(theme::PANEL);
    let mut lines = Vec::new();
    let mut links = Vec::new();

    for (row_idx, row) in render_message_body(body, width).into_iter().enumerate() {
        let mut col = 2u16;
        let mut content = Vec::new();
        for run in row {
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
                    style: run.style,
                });
            }
            col = col.saturating_add(width);
            content.push(Span::styled(run.text, run.style));
        }
        lines.push(message_card_line(gutter, content));
    }
    let mut meta = vec![Span::styled(
        format!("@{}", author),
        theme::message_author(is_current_user),
    )];
    if let Some(created_at) = created_at.and_then(format_message_created_at) {
        meta.push(Span::styled(
            format!(" · {created_at}"),
            theme::message_meta(),
        ));
    }
    if edited_at.is_some() {
        meta.push(Span::styled(" · edited", theme::message_meta()));
    }
    if let Some(reactions) = reactions.filter(|value| !value.is_empty()) {
        meta.push(Span::styled(
            format!(" · {reactions}"),
            theme::message_meta(),
        ));
    }
    lines.push(message_card_line(gutter, meta));

    MessageCard {
        item: ListItem::new(lines).style(theme::message_card()),
        links,
    }
}

fn append_plain_item<'a>(items: &mut Vec<ListItem<'a>>, content_row: &mut u16, item: ListItem<'a>) {
    *content_row = content_row.saturating_add(item.height().min(u16::MAX as usize) as u16);
    items.push(item);
}

fn append_message_card<'a>(
    items: &mut Vec<ListItem<'a>>,
    link_hits: &mut Vec<MessageLinkHit>,
    content_row: &mut u16,
    card: MessageCard<'a>,
) {
    for mut link in card.links {
        link.row = link.row.saturating_add(*content_row);
        link_hits.push(link);
    }
    *content_row = content_row.saturating_add(card.item.height().min(u16::MAX as usize) as u16);
    items.push(card.item);
}

fn register_link_hits(ui: &mut UiState, area: Rect, link_hits: Vec<MessageLinkHit>, offset_y: u16) {
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

fn format_message_created_at(created_at: &str) -> Option<String> {
    format_message_created_at_at(created_at, OffsetDateTime::now_utc())
}

fn format_message_created_at_at(created_at: &str, now: OffsetDateTime) -> Option<String> {
    let created_at =
        OffsetDateTime::parse(created_at, &time::format_description::well_known::Rfc3339).ok()?;
    let seconds = (now - created_at).whole_seconds().max(0);
    match seconds {
        0..=59 => Some("just now".to_string()),
        60..=3_599 => Some(format!("{}m ago", seconds / 60)),
        3_600..=86_399 => Some(format!("{}h ago", seconds / 3_600)),
        86_400..=604_799 => Some(format!("{}d ago", seconds / 86_400)),
        _ => created_at
            .format(format_description!(
                "[month repr:short] [day padding:none], [year] [hour]:[minute] UTC"
            ))
            .ok(),
    }
}

fn message_gap<'a>() -> ListItem<'a> {
    ListItem::new(Line::from("")).style(theme::panel())
}

fn message_card_line<'a>(gutter: Style, content: Vec<Span<'a>>) -> Line<'a> {
    let mut spans = vec![Span::styled("│ ", gutter)];
    spans.extend(content);
    Line::from(spans)
}

fn message_content_width(area: Rect) -> usize {
    area.width.saturating_sub(4).max(8) as usize
}

