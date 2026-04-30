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
    input_lines.min(5) + 3
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

    let mut text = sshoosh_splash_logo_lines(area);
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
        "                    _##                           ###",
        "                   _##^                          ###^",
        "    _####################__######_#######_#############_",
        "   _###____###____### ######^_###### _######____### ###^",
        "   _######_######### #######_#######_###_######### ###^",
        "  ######^######^###^_###^#####^^^#####^######^### _###",
        "_#################^ ############################^ ##^",
    ];
    logo_lines(LOGO)
}

fn sshoosh_splash_logo_lines(area: Rect) -> Vec<Line<'static>> {
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

    if {
        let width = STARTUP.iter().map(|line| line.len()).max().unwrap_or(0) as u16;
        let height = STARTUP.len() as u16 + 2;
        width <= area.width && height <= area.height
    } {
        logo_lines(STARTUP)
    } else {
        sshoosh_logo_lines()
    }
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
        .fg(theme::SUBTLE)
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

pub(crate) fn draw_topbar(
    frame: &mut Frame,
    area: Rect,
    _account: &Account,
    _snapshot: &Snapshot,
    _ui: &mut UiState,
) {
    if area.is_empty() {
        return;
    }
    frame.render_widget(Block::default().style(theme::topbar()), area);
    let content_y = area.y.saturating_add(area.height.saturating_sub(1) / 2);
    let content_x = area.x.saturating_add(1);
    let content_width = area.width.saturating_sub(2);
    if content_width == 0 {
        return;
    }

    let logo = "sshoosh";
    let logo_width = char_width(logo);
    frame.render_widget(
        Paragraph::new(Line::from(Span::styled(
            logo,
            theme::topbar_tab()
                .add_modifier(Modifier::BOLD)
                .add_modifier(Modifier::ITALIC),
        )))
        .style(theme::topbar()),
        Rect::new(
            content_x,
            content_y,
            (logo_width as u16).min(content_width),
            1,
        ),
    );

    let workspace = "workspace main";
    let workspace_width = char_width(workspace);
    if logo_width.saturating_add(1).saturating_add(workspace_width) > content_width as usize {
        return;
    }
    frame.render_widget(
        Paragraph::new(Line::from(vec![
            Span::styled("workspace ", theme::topbar().fg(theme::MUTED)),
            Span::styled(
                "main",
                theme::topbar().fg(theme::TEXT).add_modifier(Modifier::BOLD),
            ),
        ]))
        .style(theme::topbar()),
        Rect::new(
            content_x.saturating_add(content_width.saturating_sub(workspace_width as u16)),
            content_y,
            workspace_width as u16,
            1,
        ),
    );
}

pub(crate) fn char_width(value: &str) -> usize {
    value.chars().count()
}

pub(crate) fn draw_horizontal_divider(frame: &mut Frame, area: Rect, color: ratatui::style::Color) {
    draw_horizontal_divider_with_bg(frame, area, color, theme::BG);
}

pub(crate) fn draw_horizontal_divider_with_bg(
    frame: &mut Frame,
    area: Rect,
    color: ratatui::style::Color,
    bg: ratatui::style::Color,
) {
    if area.height == 0 || area.width == 0 {
        return;
    }
    frame.render_widget(
        Paragraph::new("─".repeat(area.width as usize)).style(Style::default().fg(color).bg(bg)),
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

pub(crate) fn draw_pane_divider_intersections(frame: &mut Frame, area: Rect, top_separator: Rect) {
    let Some(x) = pane_divider_x(area) else {
        return;
    };
    if top_separator.height > 0 {
        draw_divider_cell(frame, x, top_separator.y, "┬", theme::BORDER, theme::BG);
    }
}

pub(crate) fn draw_divider_cell(
    frame: &mut Frame,
    x: u16,
    y: u16,
    symbol: &'static str,
    color: ratatui::style::Color,
    bg: ratatui::style::Color,
) {
    frame.render_widget(
        Paragraph::new(symbol).style(Style::default().fg(color).bg(bg)),
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
