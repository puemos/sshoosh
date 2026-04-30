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
    let modal = centered(area, 76, 21);
    let inner = elevated_panel(frame, modal, "sshoosh setup");
    let suggested_username = account
        .pending_username
        .as_deref()
        .unwrap_or(&account.username);
    let mut text = sshoosh_logo_lines();
    text.extend([
        Line::from(""),
        Line::from(Span::styled(
            "This SSH key is not activated yet.",
            theme::elevated_unread(),
        )),
        Line::from(""),
        Line::from("Enter the bootstrap token or ask an owner/admin for an invite code."),
        Line::from("Type the secret and press Enter, or use: /join SECRET username"),
        Line::from(""),
        Line::from(vec![
            Span::styled("Suggested username: ", theme::elevated_muted()),
            Span::styled(suggested_username.to_string(), theme::elevated_accent()),
        ]),
        Line::from(""),
        Line::from(format!("> {}", ui.composer.buffer)),
    ]);
    frame.render_widget(
        Paragraph::new(text)
            .style(theme::elevated_panel())
            .wrap(Wrap { trim: true }),
        inner,
    );
    let input = Rect::new(inner.x, inner.y.saturating_add(14), inner.width, 1);
    ui.hit_map
        .push(input, HitTarget::ComposerInput { scroll_y: 0 });
}

pub(crate) fn draw_startup_splash(frame: &mut Frame, area: Rect, ui: &mut UiState) {
    frame.render_widget(Clear, area);
    frame.render_widget(Block::default().style(theme::elevated_panel()), area);
    ui.hit_map.push(area, HitTarget::BannerModal);

    let mut text = sshoosh_logo_lines();
    text.extend([
        Line::from(""),
        Line::from(Span::styled(
            "SSH workspace chat, served as a terminal.",
            theme::elevated_muted(),
        )),
    ]);
    let splash_height = text.len() as u16;
    let inner = Rect::new(
        area.x,
        area.y
            .saturating_add(area.height.saturating_sub(splash_height) / 2),
        area.width,
        splash_height.min(area.height),
    );
    frame.render_widget(
        Paragraph::new(text)
            .style(theme::elevated_panel())
            .alignment(Alignment::Center),
        inner,
    );
}

pub(crate) fn sshoosh_logo_lines() -> Vec<Line<'static>> {
    const LOGO: &[&str] = &[
        "                       _##                              _##",
        "                      ###^                             _###",
        "     _________________###____ _______  _______ ________####___",
        "    _###^^^^####^^^^####^#######^^#######^#######^^^^^###^^###^",
        "    ####### ^######_### _######^ #######  ###^#######_##^ _###",
        "   _____###_____######^ #######__#######_####_____###### _###",
        "  ###################^ ####_#######_#######_#######_###  ###",
        " ###################^ ^###############################  ###^",
    ];
    LOGO.iter().map(|line| pixel_logo_line(line)).collect()
}

fn pixel_logo_line(pattern: &'static str) -> Line<'static> {
    let fill = Style::default()
        .fg(theme::SUBTLE)
        .add_modifier(Modifier::BOLD);
    let spans = pattern
        .chars()
        .map(|ch| match ch {
            '#' => Span::styled("█", fill),
            '^' => Span::styled("▀", fill),
            '_' => Span::styled("▄", fill),
            _ => Span::raw(" "),
        })
        .collect::<Vec<_>>();
    Line::from(spans)
}

pub(crate) fn draw_topbar(
    frame: &mut Frame,
    area: Rect,
    account: &Account,
    snapshot: &Snapshot,
    ui: &mut UiState,
) {
    if area.is_empty() {
        return;
    }
    frame.render_widget(Block::default().style(theme::topbar()), area);
    let logo = "sshoosh";
    frame.render_widget(
        Paragraph::new(Line::from(Span::styled(
            logo,
            theme::topbar_tab()
                .add_modifier(Modifier::BOLD)
                .add_modifier(Modifier::ITALIC),
        )))
        .style(theme::topbar()),
        Rect::new(area.x, area.y, logo.chars().count() as u16, 1),
    );

    let unread = snapshot.total_unread();
    let unread_text = format!(" unread:{unread}");
    let notifications_text = format!(" notifications:{}", snapshot.notification_unread_count);
    let mentions_text = format!(" mentions:{}", snapshot.mention_unread_count);
    let online_text = format!("  {} online", snapshot.online_user_count());
    let account_text = format!("  {} ({})", account.username, account.role.as_str());
    let logo_width = char_width(logo);
    let max_cluster_width = (area.width as usize).saturating_sub(logo_width.saturating_add(1));
    if max_cluster_width == 0 {
        return;
    }
    let active_full = active_label(snapshot, ui);
    let active_full_width = char_width(&active_full).saturating_add(1);
    let mut show_notifications = true;
    let mut show_mentions = true;
    let mut show_online = true;
    let mut show_account = true;
    let mut fixed_width = topbar_fixed_width(
        &unread_text,
        show_notifications.then_some(notifications_text.as_str()),
        show_mentions.then_some(mentions_text.as_str()),
        show_online.then_some(online_text.as_str()),
        show_account.then_some(account_text.as_str()),
    );
    if fixed_width.saturating_add(active_full_width) > max_cluster_width {
        show_account = false;
        fixed_width = topbar_fixed_width(
            &unread_text,
            show_notifications.then_some(notifications_text.as_str()),
            show_mentions.then_some(mentions_text.as_str()),
            show_online.then_some(online_text.as_str()),
            show_account.then_some(account_text.as_str()),
        );
    }
    if fixed_width.saturating_add(active_full_width) > max_cluster_width {
        show_online = false;
        fixed_width = topbar_fixed_width(
            &unread_text,
            show_notifications.then_some(notifications_text.as_str()),
            show_mentions.then_some(mentions_text.as_str()),
            show_online.then_some(online_text.as_str()),
            show_account.then_some(account_text.as_str()),
        );
    }
    if snapshot.notification_unread_count == 0
        && fixed_width.saturating_add(active_full_width) > max_cluster_width
    {
        show_notifications = false;
        fixed_width = topbar_fixed_width(
            &unread_text,
            show_notifications.then_some(notifications_text.as_str()),
            show_mentions.then_some(mentions_text.as_str()),
            show_online.then_some(online_text.as_str()),
            show_account.then_some(account_text.as_str()),
        );
    }
    if snapshot.mention_unread_count == 0
        && fixed_width.saturating_add(active_full_width) > max_cluster_width
    {
        show_mentions = false;
        fixed_width = topbar_fixed_width(
            &unread_text,
            show_notifications.then_some(notifications_text.as_str()),
            show_mentions.then_some(mentions_text.as_str()),
            show_online.then_some(online_text.as_str()),
            show_account.then_some(account_text.as_str()),
        );
    }
    let active_budget = max_cluster_width.saturating_sub(fixed_width);
    let active = if active_budget > 1 {
        format!(
            " {}",
            truncate_text(active_full, active_budget.saturating_sub(1))
        )
    } else {
        String::new()
    };
    let cluster_width = char_width(&active)
        .saturating_add(fixed_width)
        .min(max_cluster_width);
    let cluster_x = area
        .x
        .saturating_add(area.width.saturating_sub(cluster_width as u16));

    let mut spans = Vec::new();
    if !active.is_empty() {
        spans.push(Span::styled(
            active.clone(),
            theme::topbar().fg(theme::TEXT).add_modifier(Modifier::BOLD),
        ));
    }
    spans.push(Span::styled(
        unread_text.clone(),
        theme::topbar().fg(if unread > 0 {
            theme::WARN
        } else {
            theme::MUTED
        }),
    ));
    let mut cursor_x = cluster_x
        .saturating_add(char_width(&active) as u16)
        .saturating_add(char_width(&unread_text) as u16);
    if show_notifications {
        spans.push(Span::styled(
            notifications_text.clone(),
            theme::topbar().fg(if snapshot.notification_unread_count > 0 {
                theme::WARN
            } else {
                theme::MUTED
            }),
        ));
        ui.hit_map.push(
            Rect::new(cursor_x, area.y, char_width(&notifications_text) as u16, 1),
            HitTarget::TopbarNotifications,
        );
        cursor_x = cursor_x.saturating_add(char_width(&notifications_text) as u16);
    }
    if show_mentions {
        spans.push(Span::styled(
            mentions_text.clone(),
            theme::topbar().fg(if snapshot.mention_unread_count > 0 {
                theme::WARN
            } else {
                theme::MUTED
            }),
        ));
        ui.hit_map.push(
            Rect::new(cursor_x, area.y, char_width(&mentions_text) as u16, 1),
            HitTarget::TopbarMentions,
        );
    }
    if show_online {
        spans.push(Span::styled(
            online_text.clone(),
            theme::topbar().fg(theme::MUTED),
        ));
    }
    if show_account {
        spans.push(Span::styled(
            account_text.clone(),
            theme::topbar().fg(theme::MUTED),
        ));
    }

    frame.render_widget(
        Paragraph::new(Line::from(spans)).style(theme::topbar()),
        Rect::new(cluster_x, area.y, cluster_width as u16, 1),
    );
}

fn char_width(value: &str) -> usize {
    value.chars().count()
}

fn topbar_fixed_width(
    unread: &str,
    notifications: Option<&str>,
    mentions: Option<&str>,
    online: Option<&str>,
    account: Option<&str>,
) -> usize {
    char_width(unread)
        + notifications.map(char_width).unwrap_or_default()
        + mentions.map(char_width).unwrap_or_default()
        + online.map(char_width).unwrap_or_default()
        + account.map(char_width).unwrap_or_default()
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
