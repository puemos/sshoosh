use super::*;
use crate::time_format::{
    calendar_day_key, calendar_day_label, format_human_timestamp, seconds_between,
};

const GROUP_GAP_SECONDS: i64 = 5 * 60;

fn should_continue_group(
    prev_author: Option<&str>,
    prev_kind: Option<MessageKind>,
    prev_created_at: Option<&str>,
    author: &str,
    kind: MessageKind,
    created_at: Option<&str>,
) -> bool {
    let Some(prev_author) = prev_author else {
        return false;
    };
    if !prev_author.eq_ignore_ascii_case(author) {
        return false;
    }
    if matches!(prev_kind, Some(MessageKind::ThreadRoot)) || matches!(kind, MessageKind::ThreadRoot)
    {
        return false;
    }
    let (Some(prev), Some(curr)) = (prev_created_at, created_at) else {
        return true;
    };
    matches!(seconds_between(prev, curr), Some(gap) if gap.abs() <= GROUP_GAP_SECONDS)
}
pub(crate) fn draw_detail(frame: &mut Frame, area: Rect, snapshot: &Snapshot, ui: &mut UiState) {
    if matches!(ui.route, Route::Dms) {
        draw_dm_detail(frame, area, snapshot, ui);
        return;
    }
    if matches!(ui.route, Route::Search) {
        draw_search_detail(frame, area, snapshot, ui);
        return;
    }
    frame.render_widget(Block::default().style(theme::panel()), area);
    let area = pane_inner(area);
    let selected = snapshot
        .selected_thread_id
        .as_ref()
        .and_then(|id| snapshot.threads.iter().find(|thread| &thread.id == id));
    let mut items: Vec<ListItem> = Vec::new();
    let message_width = message_content_width(area);
    let channel_slug = snapshot
        .selected_channel_id
        .as_ref()
        .and_then(|id| snapshot.channels.iter().find(|channel| &channel.id == id))
        .map(|channel| channel.slug.as_str());
    let title = selected
        .map(|thread| thread.title.as_str())
        .unwrap_or("Thread");
    let header_meta = selected.map(|thread| {
        let last_activity = thread
            .last_activity_at
            .as_deref()
            .map(format_human_timestamp)
            .unwrap_or_else(|| "no activity".to_string());
        let plural = if thread.comment_count == 1 {
            "comment"
        } else {
            "comments"
        };
        let mut meta = format!("{} {plural} · {}", thread.comment_count, last_activity,);
        if thread.edited_at.is_some() {
            meta.push_str(" · edited");
        }
        if thread.archived_at.is_some() {
            meta.push_str(" · archived");
        }
        if thread.pinned_at.is_some() {
            meta.push_str(" · pinned");
        }
        meta
    });
    draw_thread_header(frame, area, channel_slug, title, header_meta.as_deref(), ui);
    let messages_area = pane_scroll_area(area);
    let mut link_hits = Vec::new();
    let mut card_hits = Vec::new();
    let mut content_row = 0u16;
    if let Some(thread) = selected {
        if snapshot.comments_has_more {
            append_plain_item(
                &mut items,
                &mut content_row,
                history_prompt("Older comments available. Use /older."),
            );
        }
        append_plain_item(&mut items, &mut content_row, ListItem::new(""));

        let mut last_day: Option<String> = None;
        let mut prev_author: Option<String> = None;
        let mut prev_kind: Option<MessageKind> = None;
        let mut prev_created_at: Option<String> = None;
        let mut first_message = true;

        if !thread.body.trim().is_empty() {
            last_day = calendar_day_key(&thread.created_at);
            let card = message_card(
                snapshot,
                MessageKind::ThreadRoot,
                HeaderMode::Full,
                &thread.author,
                Some(&thread.created_at),
                thread.edited_at.as_deref(),
                Some(&thread.reactions),
                &thread.body,
                message_width,
            );
            append_message_card(
                &mut items,
                &mut link_hits,
                &mut card_hits,
                &mut content_row,
                card,
            );
            prev_author = Some(thread.author.clone());
            prev_kind = Some(MessageKind::ThreadRoot);
            prev_created_at = Some(thread.created_at.clone());
            first_message = false;
        }
        for comment in snapshot.comments.iter() {
            let day = calendar_day_key(&comment.created_at);
            let day_changed = day.is_some() && last_day.is_some() && day != last_day;
            let continue_group = !day_changed
                && should_continue_group(
                    prev_author.as_deref(),
                    prev_kind,
                    prev_created_at.as_deref(),
                    &comment.author,
                    MessageKind::Comment,
                    Some(&comment.created_at),
                );
            if day_changed && let Some(label) = calendar_day_label(&comment.created_at) {
                append_plain_item(&mut items, &mut content_row, message_gap());
                append_plain_item(
                    &mut items,
                    &mut content_row,
                    date_divider(&label, message_width),
                );
                append_plain_item(&mut items, &mut content_row, message_gap());
            } else if !continue_group && !first_message {
                append_plain_item(&mut items, &mut content_row, message_gap());
            }
            if day.is_some() {
                last_day = day;
            }
            let header_mode = if continue_group {
                HeaderMode::Suppressed
            } else {
                HeaderMode::Full
            };
            let card = message_card(
                snapshot,
                MessageKind::Comment,
                header_mode,
                &comment.author,
                Some(&comment.created_at),
                comment.edited_at.as_deref(),
                Some(&comment.reactions),
                &comment.body,
                message_width,
            );
            let card = with_message_card_hit(
                card,
                HitTarget::EditableMessage(EditableMessageTarget::Comment(comment.obj_index)),
            );
            append_message_card(
                &mut items,
                &mut link_hits,
                &mut card_hits,
                &mut content_row,
                card,
            );
            prev_author = Some(comment.author.clone());
            prev_kind = Some(MessageKind::Comment);
            prev_created_at = Some(comment.created_at.clone());
            first_message = false;
        }
    } else {
        ui.hit_map.push(messages_area, HitTarget::DetailScroll);
        render_empty_state(
            frame,
            messages_area,
            &mut ui.detail_scroll,
            empty_thread_lines(snapshot),
        );
        return;
    }
    ui.hit_map.push(messages_area, HitTarget::DetailScroll);
    render_scroll_items(frame, messages_area, items, &mut ui.detail_scroll);
    register_card_hits(ui, messages_area, card_hits, ui.detail_scroll.offset().y);
    register_link_hits(ui, messages_area, link_hits, ui.detail_scroll.offset().y);
}

pub(crate) fn draw_search_detail(
    frame: &mut Frame,
    area: Rect,
    snapshot: &Snapshot,
    ui: &mut UiState,
) {
    frame.render_widget(Block::default().style(theme::panel()), area);
    let area = pane_inner(area);
    let title = snapshot
        .search_query
        .as_ref()
        .map(|query| format!("Search: {query}"))
        .unwrap_or_else(|| "Search".to_string());
    draw_detail_header(frame, area, &title, ui);
    let messages_area = pane_scroll_area(area);
    let mut items = Vec::new();
    if snapshot.search_results.is_empty() {
        ui.hit_map.push(messages_area, HitTarget::DetailScroll);
        render_empty_state(
            frame,
            messages_area,
            &mut ui.detail_scroll,
            empty_search_lines(snapshot.search_query.as_deref()),
        );
        return;
    } else {
        for (idx, result) in snapshot.search_results.iter().enumerate() {
            let selected = idx == ui.search_selected;
            let style = if selected {
                theme::title()
            } else {
                theme::message_body()
            };
            let kind = match result.kind {
                SearchKind::Thread => "thread",
                SearchKind::Comment => "comment",
                SearchKind::Dm => "dm",
            };
            items.push(ListItem::new(vec![
                Line::from(vec![
                    Span::styled(format!("{:<8}", kind), theme::muted()),
                    Span::styled(sanitize_terminal_visible_text(&result.label), style),
                ]),
                Line::from(vec![
                    Span::styled(
                        format!("{}  ", sanitize_terminal_visible_text(&result.context)),
                        theme::muted(),
                    ),
                    Span::raw(sanitize_terminal_visible_text(&result.snippet)),
                ]),
                Line::from(""),
            ]));
        }
        if snapshot.search_has_more {
            items.push(history_prompt("More results available. Use /more."));
        }
    }
    ui.hit_map.push(messages_area, HitTarget::DetailScroll);
    render_scroll_items(frame, messages_area, items, &mut ui.detail_scroll);
}

pub(crate) fn draw_workspace_header(frame: &mut Frame, area: Rect, title: &str, ui: &UiState) {
    draw_pane_header(
        frame,
        area,
        title,
        theme::section_header(matches!(&ui.route, Route::Channel(_))),
    );
}

pub(crate) fn draw_thread_header(
    frame: &mut Frame,
    area: Rect,
    channel_slug: Option<&str>,
    title: &str,
    meta: Option<&str>,
    ui: &UiState,
) {
    if area.height == 0 || area.width == 0 {
        return;
    }
    let header = Rect::new(area.x, area.y, area.width, 1);
    let active = ui.active_pane == ActivePane::Detail;
    let title_style = if active {
        theme::title()
    } else {
        theme::muted()
    };
    let mut spans = Vec::new();
    if let Some(slug) = channel_slug {
        spans.push(Span::styled(
            format!("#{}", sanitize_terminal_visible_text(slug)),
            title_style,
        ));
        spans.push(Span::styled(" › ", theme::muted()));
    }
    spans.push(Span::styled(
        sanitize_terminal_visible_text(title),
        title_style,
    ));
    if let Some(meta) = meta.filter(|value| !value.is_empty()) {
        let used: usize = spans.iter().map(|span| span.content.chars().count()).sum();
        let remaining = (area.width as usize).saturating_sub(used);
        if remaining > 6 {
            spans.push(Span::styled("   ", theme::muted()));
            let meta_max = remaining.saturating_sub(3);
            let truncated = truncate_text(meta, meta_max);
            spans.push(Span::styled(truncated, theme::muted()));
        }
    }
    frame.render_widget(
        Paragraph::new(Line::from(spans)).style(theme::panel()),
        header,
    );
}

pub(crate) fn draw_detail_header(frame: &mut Frame, area: Rect, title: &str, ui: &UiState) {
    draw_pane_header(
        frame,
        area,
        title,
        if ui.active_pane == ActivePane::Detail {
            theme::title()
        } else {
            theme::muted()
        },
    );
}

pub(crate) fn draw_pane_header(frame: &mut Frame, area: Rect, title: &str, style: Style) {
    if area.height == 0 || area.width == 0 {
        return;
    }
    let header = Rect::new(area.x, area.y, area.width, 1);
    frame.render_widget(
        Paragraph::new(Line::from(Span::styled(
            sanitize_terminal_visible_text(title),
            style,
        )))
        .style(theme::panel()),
        header,
    );
}

pub(crate) fn pane_scroll_area(area: Rect) -> Rect {
    let header_height = area.height.min(1);
    Rect::new(
        area.x,
        area.y.saturating_add(header_height),
        area.width,
        area.height.saturating_sub(header_height),
    )
}

pub(crate) fn ensure_scroll_row_visible(
    state: &mut ScrollViewState,
    row: Option<u16>,
    viewport_height: u16,
) {
    let Some(row) = row else {
        return;
    };
    if viewport_height == 0 {
        return;
    }
    let offset = state.offset();
    let bottom = offset.y.saturating_add(viewport_height);
    let next_y = if row < offset.y {
        row
    } else if row >= bottom {
        row.saturating_add(1).saturating_sub(viewport_height)
    } else {
        offset.y
    };
    if next_y != offset.y || offset.x != 0 {
        state.set_offset(Position { x: 0, y: next_y });
    }
}

pub(crate) fn register_scroll_hits(
    ui: &mut UiState,
    area: Rect,
    scroll_target: HitTarget,
    row_hits: Vec<(u16, HitTarget)>,
    offset_y: u16,
) {
    ui.hit_map.push(area, scroll_target);
    let bottom = offset_y.saturating_add(area.height);
    for (row, target) in row_hits {
        if row < offset_y || row >= bottom {
            continue;
        }
        ui.hit_map.push(
            Rect::new(area.x, area.y + row.saturating_sub(offset_y), area.width, 1),
            target,
        );
    }
}

pub(crate) fn render_scroll_items(
    frame: &mut Frame,
    area: Rect,
    items: Vec<ListItem>,
    state: &mut ScrollViewState,
) {
    if area.is_empty() {
        return;
    }
    let content_height = items
        .iter()
        .map(ListItem::height)
        .sum::<usize>()
        .max(1)
        .min(u16::MAX as usize) as u16;
    let mut scroll_view = ScrollView::new(Size::new(area.width, content_height))
        .vertical_scrollbar_visibility(ScrollbarVisibility::Automatic)
        .horizontal_scrollbar_visibility(ScrollbarVisibility::Never);
    scroll_view.render_widget(
        List::new(items)
            .style(theme::panel())
            .highlight_style(theme::panel()),
        Rect::new(0, 0, area.width, content_height),
    );
    frame.render_stateful_widget(scroll_view, area, state);
}

pub(crate) fn draw_dm_detail(frame: &mut Frame, area: Rect, snapshot: &Snapshot, ui: &mut UiState) {
    frame.render_widget(Block::default().style(theme::panel()), area);
    let area = pane_inner(area);
    let title = snapshot
        .selected_conversation_id
        .as_ref()
        .and_then(|id| snapshot.conversations.iter().find(|dm| &dm.id == id))
        .map(|dm| format!("DM @{}", dm.peer_username))
        .unwrap_or_else(|| "DMs".to_string());
    draw_detail_header(frame, area, &title, ui);
    let messages_area = pane_scroll_area(area);
    let message_width = message_content_width(area);
    let mut items: Vec<ListItem> = Vec::new();
    let mut link_hits = Vec::new();
    let mut card_hits = Vec::new();
    let mut content_row = 0u16;
    let selected = snapshot
        .selected_conversation_id
        .as_ref()
        .and_then(|id| snapshot.conversations.iter().find(|dm| &dm.id == id));
    if snapshot.conversation_messages_has_more {
        append_plain_item(
            &mut items,
            &mut content_row,
            history_prompt("Older messages available. Use /older."),
        );
    }
    if snapshot.conversation_messages.is_empty() {
        ui.hit_map.push(messages_area, HitTarget::DetailScroll);
        render_empty_state(
            frame,
            messages_area,
            &mut ui.detail_scroll,
            empty_dm_lines(selected.is_some()),
        );
        return;
    } else {
        let mut prev_author: Option<String> = None;
        let mut prev_kind: Option<MessageKind> = None;
        let mut prev_created_at: Option<String> = None;
        for (idx, message) in snapshot.conversation_messages.iter().enumerate() {
            let continue_group = should_continue_group(
                prev_author.as_deref(),
                prev_kind,
                prev_created_at.as_deref(),
                &message.author,
                MessageKind::Dm,
                Some(&message.created_at),
            );
            if idx > 0 && !continue_group {
                append_plain_item(&mut items, &mut content_row, message_gap());
            }
            let header_mode = if continue_group {
                HeaderMode::Suppressed
            } else {
                HeaderMode::Full
            };
            let card = message_card(
                snapshot,
                MessageKind::Dm,
                header_mode,
                &message.author,
                Some(&message.created_at),
                message.edited_at.as_deref(),
                Some(&message.reactions),
                &message.body,
                message_width,
            );
            let card = with_message_card_hit(
                card,
                HitTarget::EditableMessage(EditableMessageTarget::Dm(message.obj_index)),
            );
            append_message_card(
                &mut items,
                &mut link_hits,
                &mut card_hits,
                &mut content_row,
                card,
            );
            prev_author = Some(message.author.clone());
            prev_kind = Some(MessageKind::Dm);
            prev_created_at = Some(message.created_at.clone());
        }
    }
    ui.hit_map.push(messages_area, HitTarget::DetailScroll);
    render_scroll_items(frame, messages_area, items, &mut ui.detail_scroll);
    register_card_hits(ui, messages_area, card_hits, ui.detail_scroll.offset().y);
    register_link_hits(ui, messages_area, link_hits, ui.detail_scroll.offset().y);
}

pub(crate) fn history_prompt(text: &'static str) -> ListItem<'static> {
    ListItem::new(Line::from(Span::styled(text, theme::muted())))
}

fn render_empty_state(
    frame: &mut Frame,
    area: Rect,
    state: &mut ScrollViewState,
    lines: Vec<Line<'static>>,
) {
    if area.is_empty() {
        return;
    }
    state.set_offset(Position { x: 0, y: 0 });
    let height = (lines.len() as u16).min(area.height);
    let y = area
        .y
        .saturating_add(area.height.saturating_sub(height) / 3);
    frame.render_widget(
        Paragraph::new(lines)
            .style(theme::panel())
            .alignment(Alignment::Center)
            .wrap(Wrap { trim: true }),
        Rect::new(area.x, y, area.width, height),
    );
}

fn empty_thread_lines(snapshot: &Snapshot) -> Vec<Line<'static>> {
    if snapshot.channels.is_empty() {
        return empty_state_lines(
            "No channels yet",
            "Create a place for the first thread",
            "/channel new name",
        );
    }
    if snapshot.threads.is_empty() {
        return empty_state_lines(
            "No threads in this channel",
            "Start the conversation here",
            "/thread new title",
        );
    }
    empty_state_lines(
        "Select a thread",
        "Browse threads on the left",
        "/thread new title",
    )
}

fn empty_search_lines(query: Option<&str>) -> Vec<Line<'static>> {
    if query.is_some_and(|value| !value.trim().is_empty()) {
        return empty_state_lines("No results", "Try different terms", "/search query");
    }
    empty_state_lines(
        "Search messages",
        "Find threads, comments, and DMs",
        "/search query",
    )
}

fn empty_dm_lines(has_selected_dm: bool) -> Vec<Line<'static>> {
    if has_selected_dm {
        return empty_state_lines(
            "No messages yet",
            "Type below to start the DM",
            "/dm open @user",
        );
    }
    empty_state_lines(
        "Select a DM",
        "Open an existing conversation",
        "/dm open @user",
    )
}

fn empty_state_lines(
    title: &'static str,
    detail: &'static str,
    command: &'static str,
) -> Vec<Line<'static>> {
    vec![
        Line::from(Span::styled(title, theme::title())),
        Line::from(""),
        Line::from(Span::styled(detail, theme::muted())),
        Line::from(vec![
            Span::styled("Use ", theme::muted()),
            Span::styled(command, theme::accent()),
        ]),
    ]
}
