use super::*;
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
    let title = selected
        .map(|thread| thread.title.as_str())
        .unwrap_or("No thread selected");
    draw_detail_header(frame, area, title, ui);
    let messages_area = pane_scroll_area(area);
    let mut link_hits = Vec::new();
    let mut content_row = 0u16;
    if let Some(thread) = selected {
        if snapshot.comments_has_more {
            append_plain_item(
                &mut items,
                &mut content_row,
                history_prompt("Older comments available. Use /older."),
            );
        }
        let summary = ListItem::new(vec![
            Line::from(vec![Span::styled(
                format!(
                    "@{} · {} comments · {}{}{}{}",
                    thread.author,
                    thread.comment_count,
                    thread.last_activity_at.as_deref().unwrap_or("no activity"),
                    if thread.edited_at.is_some() {
                        " · edited"
                    } else {
                        ""
                    },
                    if thread.archived_at.is_some() {
                        " · archived"
                    } else {
                        ""
                    },
                    if thread.pinned_at.is_some() {
                        " · pinned"
                    } else {
                        ""
                    }
                ),
                theme::muted(),
            )]),
            Line::from(""),
        ]);
        append_plain_item(&mut items, &mut content_row, summary);

        if !thread.body.trim().is_empty() {
            let card = message_card(
                snapshot,
                &thread.author,
                Some(&thread.created_at),
                thread.edited_at.as_deref(),
                Some(&thread.reactions),
                &thread.body,
                message_width,
            );
            append_message_card(&mut items, &mut link_hits, &mut content_row, card);
        }
        for (idx, comment) in snapshot.comments.iter().enumerate() {
            if idx == 0 {
                append_plain_item(&mut items, &mut content_row, message_gap());
            }
            let card = message_card(
                snapshot,
                &comment.author,
                Some(&comment.created_at),
                comment.edited_at.as_deref(),
                Some(&comment.reactions),
                &comment.body,
                message_width,
            );
            append_message_card(&mut items, &mut link_hits, &mut content_row, card);
            if idx + 1 < snapshot.comments.len() {
                append_plain_item(&mut items, &mut content_row, message_gap());
            }
        }
    } else {
        items.push(ListItem::new(vec![
            Line::from("No thread selected."),
            Line::from(Span::styled(
                "Create one with /thread new title",
                theme::muted(),
            )),
        ]));
    }
    ui.hit_map.push(messages_area, HitTarget::DetailScroll);
    render_scroll_items(frame, messages_area, items, &mut ui.detail_scroll);
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
        items.push(ListItem::new(vec![
            Line::from("No results."),
            Line::from(Span::styled("Use /search query", theme::muted())),
        ]));
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
                    Span::styled(result.label.clone(), style),
                ]),
                Line::from(vec![
                    Span::styled(format!("{}  ", result.context), theme::muted()),
                    Span::raw(result.snippet.clone()),
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
        Paragraph::new(Line::from(Span::styled(title.to_string(), style))).style(theme::panel()),
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
    let mut content_row = 0u16;
    if snapshot.conversation_messages_has_more {
        append_plain_item(
            &mut items,
            &mut content_row,
            history_prompt("Older messages available. Use /older."),
        );
    }
    if snapshot.conversation_messages.is_empty() {
        items.push(ListItem::new(vec![
            Line::from("No messages yet."),
            Line::from(Span::styled(
                "Type a message or use /dm open @user",
                theme::muted(),
            )),
        ]));
    } else {
        for (idx, message) in snapshot.conversation_messages.iter().enumerate() {
            if idx > 0 {
                append_plain_item(&mut items, &mut content_row, message_gap());
            }
            let card = message_card(
                snapshot,
                &message.author,
                Some(&message.created_at),
                message.edited_at.as_deref(),
                Some(&message.reactions),
                &message.body,
                message_width,
            );
            append_message_card(&mut items, &mut link_hits, &mut content_row, card);
        }
    }
    ui.hit_map.push(messages_area, HitTarget::DetailScroll);
    render_scroll_items(frame, messages_area, items, &mut ui.detail_scroll);
    register_link_hits(ui, messages_area, link_hits, ui.detail_scroll.offset().y);
}

pub(crate) fn history_prompt(text: &'static str) -> ListItem<'static> {
    ListItem::new(Line::from(Span::styled(text, theme::muted())))
}
