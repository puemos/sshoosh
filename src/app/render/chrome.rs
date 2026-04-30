use super::*;
pub(crate) fn bottombar_height(ui: &UiState) -> u16 {
    let input_lines = if ui.mode == UiMode::Compose {
        ui.composer
            .buffer
            .bytes()
            .filter(|byte| *byte == b'\n')
            .count() as u16
            + 1
    } else {
        1
    };
    input_lines.min(5) + 4
}

pub(crate) fn draw_onboarding(frame: &mut Frame, area: Rect, account: &Account, ui: &mut UiState) {
    let modal = centered(area, 72, 13);
    let block = panel(" sshoosh setup ", true);
    let suggested_username = account
        .pending_username
        .as_deref()
        .unwrap_or(&account.username);
    let text = vec![
        Line::from(Span::styled(
            "This SSH key is not activated yet.",
            theme::unread(),
        )),
        Line::from(""),
        Line::from("Enter the bootstrap token or ask an owner/admin for an invite code."),
        Line::from("Type the secret and press Enter, or use: /join SECRET username"),
        Line::from(""),
        Line::from(vec![
            Span::styled("Suggested username: ", theme::muted()),
            Span::styled(suggested_username.to_string(), theme::accent()),
        ]),
        Line::from(""),
        Line::from(format!("> {}", ui.composer.buffer)),
    ];
    frame.render_widget(
        Paragraph::new(text)
            .style(theme::panel())
            .block(block.padding(Padding::uniform(1)))
            .wrap(Wrap { trim: true }),
        modal,
    );
    let input = Rect::new(
        modal.x.saturating_add(2),
        modal.y.saturating_add(9),
        modal.width.saturating_sub(4),
        1,
    );
    ui.hit_map
        .push(input, HitTarget::ComposerInput { scroll_y: 0 });
}

pub(crate) fn draw_topbar(
    frame: &mut Frame,
    area: Rect,
    account: &Account,
    snapshot: &Snapshot,
    ui: &UiState,
) {
    let active = active_label(snapshot, ui);
    let unread = snapshot.total_unread();
    let line = Line::from(vec![
        Span::styled(" sshoosh", theme::topbar_tab().add_modifier(Modifier::BOLD)),
        Span::styled(
            format!(" {active}"),
            theme::topbar().fg(theme::TEXT).add_modifier(Modifier::BOLD),
        ),
        Span::styled(
            format!(" unread:{unread}"),
            theme::topbar().fg(if unread > 0 {
                theme::WARN
            } else {
                theme::MUTED
            }),
        ),
        Span::styled(
            format!(
                "  notifications:{} mentions:{}",
                snapshot.notification_unread_count, snapshot.mention_unread_count
            ),
            theme::topbar().fg(if snapshot.notification_unread_count > 0 {
                theme::WARN
            } else {
                theme::MUTED
            }),
        ),
        Span::styled(
            format!("  {} online", snapshot.online_user_count()),
            theme::topbar().fg(theme::MUTED),
        ),
        Span::styled(
            format!("  {} ({})", account.username, account.role.as_str()),
            theme::topbar().fg(theme::MUTED),
        ),
    ]);
    frame.render_widget(Paragraph::new(line).style(theme::topbar()), area);
}

pub(crate) fn draw_horizontal_divider(frame: &mut Frame, area: Rect, color: ratatui::style::Color) {
    if area.height == 0 || area.width == 0 {
        return;
    }
    frame.render_widget(
        Paragraph::new("─".repeat(area.width as usize))
            .style(Style::default().fg(color).bg(theme::BG)),
        area,
    );
}

pub(crate) fn active_label(snapshot: &Snapshot, ui: &UiState) -> String {
    match &ui.route {
        Route::Channel(id) => snapshot
            .channels
            .iter()
            .find(|channel| &channel.id == id)
            .map(|channel| channel_label(&channel.visibility, &channel.slug))
            .unwrap_or_else(|| "#channel".to_string()),
        Route::Dms => snapshot
            .selected_conversation_id
            .as_ref()
            .and_then(|id| snapshot.conversations.iter().find(|dm| &dm.id == id))
            .map(|dm| format!("@{}", dm.peer_username))
            .unwrap_or_else(|| "DMs".to_string()),
        Route::Search => snapshot
            .search_query
            .as_ref()
            .map(|query| format!("Search: {query}"))
            .unwrap_or_else(|| "Search".to_string()),
    }
}

pub(crate) fn draw_body(frame: &mut Frame, area: Rect, snapshot: &Snapshot, ui: &mut UiState) {
    if area.width >= 80 {
        let cols = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([
                Constraint::Length(WORKSPACE_PANE_WIDTH),
                Constraint::Length(1),
                Constraint::Min(40),
            ])
            .split(area);
        draw_workspace(frame, cols[0], snapshot, ui);
        draw_vertical_divider(frame, cols[1]);
        draw_detail(frame, cols[2], snapshot, ui);
    } else {
        match ui.active_pane {
            ActivePane::Rail | ActivePane::List => draw_workspace(frame, area, snapshot, ui),
            ActivePane::Detail => draw_detail(frame, area, snapshot, ui),
        }
    }
}

pub(crate) fn pane_divider_x(area: Rect) -> Option<u16> {
    (area.width >= 80).then(|| area.x.saturating_add(WORKSPACE_PANE_WIDTH))
}

pub(crate) fn draw_vertical_divider(frame: &mut Frame, area: Rect) {
    if area.is_empty() {
        return;
    }
    let divider = (0..area.height).map(|_| "│").collect::<Vec<_>>().join("\n");
    frame.render_widget(
        Paragraph::new(divider).style(Style::default().fg(theme::BORDER).bg(theme::BG)),
        area,
    );
}

pub(crate) fn draw_pane_divider_intersections(
    frame: &mut Frame,
    area: Rect,
    top_separator: Rect,
    bottom_bar: Rect,
    bottom_color: ratatui::style::Color,
) {
    let Some(x) = pane_divider_x(area) else {
        return;
    };
    if top_separator.height > 0 {
        draw_divider_cell(frame, x, top_separator.y, "┬", theme::BORDER);
    }
    if bottom_bar.height > 0 {
        draw_divider_cell(frame, x, bottom_bar.y, "┴", bottom_color);
    }
}

pub(crate) fn draw_divider_cell(
    frame: &mut Frame,
    x: u16,
    y: u16,
    symbol: &'static str,
    color: ratatui::style::Color,
) {
    frame.render_widget(
        Paragraph::new(symbol).style(Style::default().fg(color).bg(theme::BG)),
        Rect::new(x, y, 1, 1),
    );
}

pub(crate) fn pane_inner(area: Rect) -> Rect {
    Rect::new(
        area.x.saturating_add(1),
        area.y.saturating_add(1),
        area.width.saturating_sub(2),
        area.height.saturating_sub(2),
    )
}
