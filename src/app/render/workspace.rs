use super::*;
pub(crate) fn draw_workspace(frame: &mut Frame, area: Rect, snapshot: &Snapshot, ui: &mut UiState) {
    frame.render_widget(Block::default().style(theme::panel()), area);
    let area = pane_inner(area);
    let row_width = area.width as usize;
    let mut items = Vec::new();
    let mut row_hits = Vec::new();
    let mut selected_y = None;
    draw_workspace_header(frame, area, "Channels", ui);
    let scroll_area = pane_scroll_area(area);
    for channel in &snapshot.channels {
        let row = items.len() as u16;
        let selected = matches!(&ui.route, Route::Channel(id) if id == &channel.id);
        let unread = snapshot.channel_unread(&channel.id);
        let unread_badge = unread_badge(unread);
        let privacy_badge = channel_privacy_badge(&channel.visibility);
        let label = truncate_text(
            format!("#{}", channel.slug),
            row_width.saturating_sub(unread_badge.len() + privacy_badge.len()),
        );
        if selected && (ui.active_pane == ActivePane::Rail || snapshot.selected_thread_id.is_none())
        {
            selected_y = Some(items.len() as u16);
        }
        items.push(ListItem::new(Line::from(vec![
            Span::styled(label, workspace_label_style(selected, unread)),
            Span::styled(privacy_badge, theme::muted()),
            Span::styled(unread_badge, theme::unread()),
        ])));
        row_hits.push((row, HitTarget::WorkspaceChannel(channel.id.clone())));

        if selected {
            if ui.threads_collapsed {
                items.push(ListItem::new(Line::from(vec![
                    Span::raw("  "),
                    Span::styled("threads hidden", theme::muted()),
                ])));
            } else if snapshot.threads.is_empty() {
                items.push(ListItem::new(Line::from(vec![
                    Span::raw("  "),
                    Span::styled("no threads", theme::muted()),
                ])));
            } else {
                let last_idx = snapshot.threads.len().saturating_sub(1);
                for (idx, thread) in snapshot.threads.iter().enumerate() {
                    let row = items.len() as u16;
                    if ui.active_pane != ActivePane::Rail
                        && snapshot.selected_thread_id.as_deref() == Some(thread.id.as_str())
                    {
                        selected_y = Some(items.len() as u16);
                    }
                    let connector = if idx == last_idx { "└─ " } else { "├─ " };
                    items.push(thread_item(snapshot, thread, row_width, connector));
                    row_hits.push((row, HitTarget::WorkspaceThread(thread.id.clone())));
                }
            }
        }
    }
    items.push(ListItem::new(""));
    items.push(ListItem::new(Line::from(Span::styled(
        "DMs",
        theme::section_header(matches!(&ui.route, Route::Dms)),
    ))));
    for dm in &snapshot.conversations {
        let row = items.len() as u16;
        let selected = snapshot.selected_conversation_id.as_deref() == Some(dm.id.as_str())
            && matches!(ui.route, Route::Dms);
        let unread_badge = unread_badge(dm.unread_count);
        let state_badge = dm_state_badge(snapshot, dm);
        let label = truncate_text(
            format!("@{}", dm.peer_username),
            row_width.saturating_sub(unread_badge.len() + state_badge.len()),
        );
        if selected {
            selected_y = Some(items.len() as u16);
        }
        items.push(ListItem::new(Line::from(vec![
            Span::styled(label, workspace_label_style(selected, dm.unread_count)),
            Span::styled(state_badge, theme::muted()),
            Span::styled(unread_badge, theme::unread()),
        ])));
        row_hits.push((row, HitTarget::WorkspaceDm(dm.id.clone())));
    }
    ensure_scroll_row_visible(&mut ui.workspace_scroll, selected_y, scroll_area.height);
    let scroll_offset_y = ui.workspace_scroll.offset().y;
    register_scroll_hits(
        ui,
        scroll_area,
        HitTarget::WorkspaceScroll,
        row_hits,
        scroll_offset_y,
    );
    render_scroll_items(frame, scroll_area, items, &mut ui.workspace_scroll);
}

pub(crate) fn channel_label(visibility: &str, slug: &str) -> String {
    if visibility == "private" {
        format!("#{slug} · private")
    } else {
        format!("#{slug}")
    }
}

pub(crate) fn channel_privacy_badge(visibility: &str) -> &'static str {
    if visibility == "private" {
        " · private"
    } else {
        ""
    }
}

pub(crate) fn thread_item<'a>(
    snapshot: &Snapshot,
    thread: &'a crate::service::ThreadItem,
    row_width: usize,
    connector: &'static str,
) -> ListItem<'a> {
    let selected = snapshot.selected_thread_id.as_deref() == Some(thread.id.as_str());
    let unread_badge = unread_badge(thread.unread_count);
    let state_badge = thread_state_badge(thread);
    let prefix_len = 2 + connector.chars().count();
    let title = truncate_text(
        &thread.title,
        row_width.saturating_sub(prefix_len + unread_badge.len() + state_badge.len()),
    );
    ListItem::new(Line::from(vec![
        Span::raw("  "),
        Span::styled(connector, theme::muted()),
        Span::styled(title, workspace_label_style(selected, thread.unread_count)),
        Span::styled(state_badge, theme::muted()),
        Span::styled(unread_badge, theme::unread()),
    ]))
}

pub(crate) fn thread_state_badge(thread: &crate::service::ThreadItem) -> String {
    let mut out = String::new();
    if thread.pinned_at.is_some() {
        out.push_str(" pin");
    }
    if thread.archived_at.is_some() {
        out.push_str(" archived");
    }
    if thread.muted_until.is_some() {
        out.push_str(" muted");
    }
    if thread.saved_at.is_some() {
        out.push_str(" saved");
    }
    if !thread.reactions.is_empty() {
        out.push(' ');
        out.push_str(&thread.reactions);
    }
    out
}

pub(crate) fn dm_state_badge(snapshot: &Snapshot, dm: &crate::service::Conversation) -> String {
    let mut out = String::new();
    out.push(' ');
    out.push_str(match snapshot.presence_for(&dm.peer_username) {
        crate::service::PresenceState::Online => "online",
        crate::service::PresenceState::Away => "away",
        crate::service::PresenceState::Offline => "offline",
    });
    if dm.muted_until.is_some() {
        out.push_str(" muted");
    }
    if dm.saved_at.is_some() {
        out.push_str(" saved");
    }
    out
}

pub(crate) fn workspace_label_style(selected: bool, unread_count: i64) -> Style {
    if selected {
        theme::title()
    } else if unread_count > 0 {
        theme::unread()
    } else {
        theme::muted()
    }
}

pub(crate) fn unread_badge(count: i64) -> String {
    if count > 0 {
        format!(" {count}")
    } else {
        String::new()
    }
}

pub(crate) fn truncate_text(value: impl AsRef<str>, max_chars: usize) -> String {
    let value = value.as_ref();
    if value.chars().count() <= max_chars {
        return value.to_string();
    }
    if max_chars == 0 {
        return String::new();
    }
    if max_chars <= 3 {
        return ".".repeat(max_chars);
    }
    let mut truncated = value.chars().take(max_chars - 3).collect::<String>();
    truncated.push_str("...");
    truncated
}
