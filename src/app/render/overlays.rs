use super::*;
pub(crate) fn draw_palette(frame: &mut Frame, full_area: Rect, area: Rect, ui: &mut UiState) {
    ui.hit_map.push(full_area, HitTarget::PaletteBackdrop);
    frame.render_widget(Clear, area);
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Length(3), Constraint::Min(3)])
        .split(area);
    frame.render_widget(
        Paragraph::new(format!("> {}", ui.palette.query))
            .style(theme::panel())
            .block(panel(" Command palette ", true)),
        chunks[0],
    );
    ui.hit_map.push(chunks[0], HitTarget::PaletteInput);
    let items: Vec<ListItem> = ui
        .palette
        .filtered
        .iter()
        .take(chunks[1].height.saturating_sub(2) as usize)
        .enumerate()
        .filter_map(|(row, idx)| ui.palette.items.get(*idx).map(|item| (row, item)))
        .map(|(row, item)| {
            let selected = row == ui.palette.selected;
            let style = if selected {
                theme::selection()
            } else {
                theme::panel()
            };
            ListItem::new(Line::from(vec![
                Span::styled(format!("{:<24}", item.label), style),
                Span::styled(format!("{:<14}", item.category), style),
                Span::styled(item.shortcut.clone().unwrap_or_default(), style),
                Span::styled("  ", style),
                Span::styled(item.detail.clone(), style),
            ]))
        })
        .collect();
    frame.render_widget(List::new(items).block(panel(" Results ", true)), chunks[1]);
    let rows = Rect::new(
        chunks[1].x.saturating_add(1),
        chunks[1].y.saturating_add(1),
        chunks[1].width.saturating_sub(2),
        chunks[1].height.saturating_sub(2),
    );
    ui.hit_map.push(rows, HitTarget::PaletteResults);
    for row in 0..ui.palette.filtered.len().min(rows.height as usize) {
        ui.hit_map.push(
            Rect::new(rows.x, rows.y + row as u16, rows.width, 1),
            HitTarget::PaletteRow(row),
        );
    }
}

pub(crate) fn draw_prompt(frame: &mut Frame, full_area: Rect, area: Rect, ui: &mut UiState) {
    ui.hit_map.push(full_area, HitTarget::PromptBackdrop);
    frame.render_widget(Clear, area);
    let text = if ui.prompt.input.is_empty() {
        format!("{}{}", ui.prompt.prefix, ui.prompt.placeholder)
    } else {
        format!("{}{}", ui.prompt.prefix, ui.prompt.input)
    };
    frame.render_widget(
        Paragraph::new(text)
            .style(theme::panel())
            .block(panel(&format!(" {} ", ui.prompt.title), true)),
        area,
    );
    ui.hit_map.push(area, HitTarget::PromptInput);
}

pub(crate) fn draw_help(
    frame: &mut Frame,
    full_area: Rect,
    area: Rect,
    commands: &[CommandSpec],
    ui: &mut UiState,
) {
    ui.hit_map.push(full_area, HitTarget::HelpBackdrop);
    frame.render_widget(Clear, area);
    let mut lines = vec![
        Line::from(Span::styled("Keyboard", theme::accent())),
        Line::from("j/k arrows move through workspace rows · h collapse/back · l open/expand"),
        Line::from("Enter open/send · Shift-Enter newline · Tab toggles workspace/detail"),
        Line::from("/ compose command · Up/Down choose suggestion · Tab accepts"),
        Line::from("Space toggles threads"),
        Line::from("Ctrl-P palette · Esc close"),
        Line::from("q quit · Ctrl-C disconnect"),
        Line::from(""),
        Line::from(Span::styled("Slash commands", theme::accent())),
    ];
    for spec in commands {
        lines.push(Line::from(vec![
            Span::styled(format!("/{:<10}", spec.name), theme::title()),
            Span::styled(format!("{:<16}", spec.args), theme::muted()),
            Span::raw(spec.description),
        ]));
    }
    frame.render_widget(
        Paragraph::new(lines)
            .style(theme::panel())
            .block(panel(" Help ", true))
            .wrap(Wrap { trim: false }),
        area,
    );
}

pub(crate) fn draw_confirm_quit(frame: &mut Frame, full_area: Rect, area: Rect, ui: &mut UiState) {
    ui.hit_map.push(full_area, HitTarget::ConfirmQuitBackdrop);
    frame.render_widget(Clear, area);
    frame.render_widget(
        Paragraph::new("Disconnect from sshoosh?  y / n")
            .alignment(Alignment::Center)
            .style(theme::panel())
            .block(panel(" Quit ", true)),
        area,
    );
    let text = "Disconnect from sshoosh?  y / n";
    let text_x = area.x + area.width.saturating_sub(text.chars().count() as u16) / 2;
    let text_y = area.y.saturating_add(1);
    ui.hit_map.push(area, HitTarget::ConfirmQuitNo);
    if let Some(y_pos) = text.find('y') {
        ui.hit_map.push(
            Rect::new(text_x + y_pos as u16, text_y, 1, 1),
            HitTarget::ConfirmQuitYes,
        );
    }
    if let Some(n_pos) = text.rfind('n') {
        ui.hit_map.push(
            Rect::new(text_x + n_pos as u16, text_y, 1, 1),
            HitTarget::ConfirmQuitNo,
        );
    }
}

pub(crate) fn draw_banner(frame: &mut Frame, area: Rect, ui: &mut UiState) {
    let Some(banner) = ui.banner.as_ref().filter(|banner| banner.active()).cloned() else {
        return;
    };
    if banner.presentation == BannerPresentation::Modal {
        draw_banner_modal(frame, area, &banner, ui);
        return;
    }

    draw_toast(frame, area, &banner, ui);
}

pub(crate) fn draw_toast(frame: &mut Frame, area: Rect, banner: &Banner, ui: &UiState) {
    if area.width < 8 || area.height < 4 {
        return;
    }

    let max_width = area.width.saturating_sub(4).clamp(1, 56);
    let text_width = banner
        .text
        .chars()
        .count()
        .saturating_add(4)
        .min(u16::MAX as usize) as u16;
    let min_width = 12.min(max_width);
    let width = text_width.max(min_width).min(max_width);
    let content_width = width.saturating_sub(4).max(1) as usize;

    let bottom_bar_top = area
        .y
        .saturating_add(area.height)
        .saturating_sub(bottombar_height(ui));
    let max_height = bottom_bar_top
        .saturating_sub(area.y)
        .saturating_sub(1)
        .min(6);
    if max_height < 3 {
        return;
    }
    let content_lines = wrapped_line_count(&banner.text, content_width);
    let height = content_lines.saturating_add(2).max(3).min(max_height);
    let x = area.x + area.width.saturating_sub(width.saturating_add(2));
    let y = bottom_bar_top.saturating_sub(height.saturating_add(1));
    let rect = Rect::new(x, y, width, height);
    let color = if banner.error {
        theme::ERROR
    } else {
        theme::OK
    };

    frame.render_widget(Clear, rect);
    frame.render_widget(
        Paragraph::new(banner.text.clone())
            .style(
                Style::default()
                    .fg(color)
                    .bg(theme::BG)
                    .add_modifier(Modifier::BOLD),
            )
            .block(
                Block::default()
                    .borders(Borders::ALL)
                    .border_style(Style::default().fg(color).bg(theme::BG))
                    .style(Style::default().fg(color).bg(theme::BG))
                    .padding(Padding::horizontal(1)),
            )
            .wrap(Wrap { trim: true }),
        rect,
    );
}

pub(crate) fn wrapped_line_count(text: &str, width: usize) -> u16 {
    let width = width.max(1);
    let lines = text
        .lines()
        .map(|line| line.chars().count().max(1).div_ceil(width))
        .sum::<usize>()
        .max(1);
    lines.min(u16::MAX as usize) as u16
}

pub(crate) fn draw_banner_modal(frame: &mut Frame, area: Rect, banner: &Banner, ui: &mut UiState) {
    let modal = centered(area, 68, 9);
    let (title, lines) = if let Some(code) = banner.text.strip_prefix("Invite code:") {
        (
            " Invite code ",
            vec![
                Line::from("One-time invite for a new SSH key"),
                Line::from(""),
                Line::from(Span::styled(
                    code.trim().to_string(),
                    Style::default().fg(theme::OK).add_modifier(Modifier::BOLD),
                )),
                Line::from(""),
                Line::from(Span::styled(
                    "c copies, Enter or Esc closes",
                    theme::muted(),
                )),
            ],
        )
    } else {
        (
            if banner.error { " Error " } else { " Message " },
            vec![Line::from(banner.text.clone())],
        )
    };
    ui.hit_map.push(modal, HitTarget::BannerModal);
    frame.render_widget(Clear, modal);
    frame.render_widget(
        Paragraph::new(lines)
            .alignment(Alignment::Center)
            .style(theme::panel())
            .block(panel(title, true).padding(Padding::uniform(1)))
            .wrap(Wrap { trim: true }),
        modal,
    );
}

pub(crate) fn panel(title: &str, active: bool) -> Block<'_> {
    Block::default()
        .title(title.to_string())
        .borders(Borders::ALL)
        .border_style(theme::border(active))
        .style(theme::panel())
}

pub(crate) fn centered(area: Rect, width: u16, height: u16) -> Rect {
    let width = width.min(area.width.saturating_sub(2)).max(1);
    let height = height.min(area.height.saturating_sub(2)).max(1);
    Rect::new(
        area.x + area.width.saturating_sub(width) / 2,
        area.y + area.height.saturating_sub(height) / 2,
        width,
        height,
    )
}
