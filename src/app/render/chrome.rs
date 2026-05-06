use super::*;
pub(crate) fn bottombar_height(ui: &UiState) -> u16 {
    let input_lines = ui
        .composer
        .buffer
        .bytes()
        .filter(|byte| *byte == b'\n')
        .count() as u16
        + 1;
    input_lines.min(5) + 3
}

pub(crate) fn draw_onboarding(frame: &mut Frame, area: Rect, _account: &Account, ui: &mut UiState) {
    let modal = centered(area, 76, 23);
    let inner = elevated_panel(frame, modal, "sshoosh setup");
    let logo = sshoosh_splash_logo_lines();
    let setup_lines = vec![
        Line::from(""),
        Line::from(Span::styled(
            "Your access token was accepted.",
            theme::elevated_unread(),
        )),
        Line::from(""),
        Line::from("Choose the username this SSH key will use in sshoosh."),
        Line::from("Use 2-32 letters, numbers, dots, dashes, or underscores."),
        Line::from(""),
        Line::from(vec![
            Span::styled("Press ", theme::elevated_muted()),
            Span::styled("Enter", theme::elevated_accent()),
            Span::styled(" to confirm.", theme::elevated_muted()),
        ]),
        Line::from(""),
        Line::from(format!(
            "username> {}",
            sanitize_terminal_visible_text(&ui.composer.buffer)
        )),
    ];
    let logo_height = logo.len() as u16;
    let setup_y = inner.y.saturating_add(logo_height);
    let input_y = setup_y.saturating_add(setup_lines.len().saturating_sub(1) as u16);

    frame.render_widget(
        Paragraph::new(logo)
            .style(theme::elevated_panel())
            .alignment(Alignment::Center),
        Rect::new(inner.x, inner.y, inner.width, logo_height.min(inner.height)),
    );
    frame.render_widget(
        Paragraph::new(setup_lines).style(theme::elevated_panel()),
        Rect::new(
            inner.x,
            setup_y,
            inner.width,
            inner.height.saturating_sub(logo_height),
        ),
    );
    let input = Rect::new(inner.x, input_y, inner.width, 1);
    ui.hit_map
        .push(input, HitTarget::ComposerInput { scroll_y: 0 });
}

pub(crate) fn draw_startup_splash(frame: &mut Frame, area: Rect, ui: &mut UiState) {
    frame.render_widget(Clear, area);
    frame.render_widget(Block::default().style(theme::elevated_panel()), area);
    ui.hit_map.push(area, HitTarget::BannerModal);

    let mut text = sshoosh_splash_logo_lines();
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

fn sshoosh_splash_logo_lines() -> Vec<Line<'static>> {
    const STARTUP: &[&str] = &[
        "                          ▗▄▄▖                                  ▗▄▄▄",
        "                          ███▘                                  ███▘",
        "                         ▟██▘                                  ▟██▌",
        "     ▗▟███████▛▄███████▛▟███████▄ ▄███████▖▗▟███████▖▗▟███████▗███████▙",
        "     ▟██▘     ▐██▛     ▗██▛  ▟██▌▟██▛  ███▘▟██▛ ▗███▚███▘     ███▘ ▐███",
        "    ▝███████▙ ▜██████▙▗███  ▗██▛▐██▛  ▟██▛▟██▛  ███▘▝███████▖▟██▌  ███▌",
        "         ███▘     ▟██▌▟██▘ ▗███▚███▘ ▐███▐███  ▐██▛     ▗███▐██▛  ▟██▛",
        "  ▗████████▘▟███████▀▟██▛  ▟██▘▝███████▛▘▜███████▛▚███████▛▚███  ▗███",
        " ▗▄▄▄▄▄▄▄▄▄▄▄▄▄▄▄▄▄▄▄██▛  ▟███▄▄▄▄▄▄▄▄▄▄▄▄▄▄▄▄▄▄▄▄▄▄▄▄▄▄▄▄▄▟██▘ ▗███▘",
        " ▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▘ ▝▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▘  ▝▀▀▘",
    ];

    logo_lines(STARTUP)
}

fn logo_lines(lines: &'static [&'static str]) -> Vec<Line<'static>> {
    let width = lines
        .iter()
        .map(|line| line.chars().count())
        .max()
        .unwrap_or_default();
    lines.iter().map(|line| logo_line(line, width)).collect()
}

fn logo_line(pattern: &'static str, width: usize) -> Line<'static> {
    let fill = Style::default()
        .fg(theme::subtle_color())
        .add_modifier(Modifier::BOLD);
    let mut spans = pattern
        .chars()
        .map(|ch| match ch {
            '#' => Span::styled("█", fill),
            '^' => Span::styled("▀", fill),
            '_' => Span::styled("▄", fill),
            '█' | '▀' | '▄' | '▌' | '▐' | '▖' | '▗' | '▘' | '▝' | '▙' | '▛' | '▜' | '▟' | '▚'
            | '▞' => Span::styled(ch.to_string(), fill),
            _ => Span::raw(" "),
        })
        .collect::<Vec<_>>();
    let padding = width.saturating_sub(pattern.chars().count());
    if padding > 0 {
        spans.push(Span::raw(" ".repeat(padding)));
    }
    Line::from(spans)
}

pub(crate) fn char_width(value: &str) -> usize {
    value.chars().count()
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
        Route::Label(tag) => format!("${tag}"),
        Route::Saved => "Saved".to_string(),
        Route::Notifications => "Notifications".to_string(),
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

pub(crate) fn draw_vertical_divider(frame: &mut Frame, area: Rect) {
    if area.is_empty() {
        return;
    }
    let divider = (0..area.height).map(|_| "│").collect::<Vec<_>>().join("\n");
    frame.render_widget(
        Paragraph::new(divider).style(Style::default().fg(theme::border()).bg(theme::bg())),
        area,
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
