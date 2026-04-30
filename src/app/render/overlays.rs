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
    let block = panel(" Help ", true).padding(Padding::uniform(1));
    let inner = block.inner(area);
    frame.render_widget(block, area);
    if inner.width == 0 || inner.height == 0 {
        return;
    }

    if inner.width >= 72 {
        let left_width = ((inner.width as u32 * 38) / 100).clamp(36, 40) as u16;
        let chunks = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([
                Constraint::Length(left_width),
                Constraint::Length(2),
                Constraint::Min(30),
            ])
            .split(inner);
        frame.render_widget(
            Paragraph::new(help_keyboard_lines(chunks[0].width as usize)).style(theme::panel()),
            chunks[0],
        );
        frame.render_widget(
            Paragraph::new(help_command_lines(commands, chunks[2].width as usize))
                .style(theme::panel()),
            chunks[2],
        );
        return;
    }

    let mut lines = help_keyboard_lines(inner.width as usize);
    lines.push(Line::from(""));
    lines.extend(help_command_lines(commands, inner.width as usize));
    frame.render_widget(Paragraph::new(lines).style(theme::panel()), inner);
}

pub(crate) fn help_modal_area(area: Rect) -> Rect {
    centered(area, 104, 24)
}

fn help_keyboard_lines(width: usize) -> Vec<Line<'static>> {
    let rows = [
        ("Navigation", "j/k", "move through workspace rows"),
        ("", "h / l", "collapse/back / open/expand"),
        ("", "Tab", "toggle workspace/detail"),
        ("", "Space", "toggle threads"),
        ("Compose", "Enter", "open selected item or send"),
        ("", "Shift-Enter", "insert newline"),
        ("", "/", "compose command"),
        ("", "Up/Down", "choose suggestion"),
        ("", "Tab", "accept suggestion"),
        ("", "Ctrl-X E", "edit latest message/comment here"),
        ("System", "Ctrl-P", "command palette"),
        ("", "Esc", "close overlay or mode"),
        ("", "q", "quit"),
        ("", "Ctrl-C", "disconnect"),
    ];
    let mut lines = vec![Line::from(Span::styled("Keyboard", theme::accent()))];
    for (group, key, description) in rows {
        lines.push(help_shortcut_line(width, group, key, description));
    }
    lines
}

fn help_shortcut_line(
    width: usize,
    group: &'static str,
    key: &'static str,
    description: &'static str,
) -> Line<'static> {
    if width >= 36 {
        let group_width = 10;
        let key_width = 12;
        let used = group_width + key_width + 2;
        let description_width = width.saturating_sub(used).max(1);
        return Line::from(vec![
            Span::styled(pad_or_truncate(group, group_width), help_group_style(group)),
            Span::raw(" "),
            Span::styled(
                pad_or_truncate(key, key_width),
                theme::muted().add_modifier(Modifier::BOLD),
            ),
            Span::raw(" "),
            Span::styled(
                pad_or_truncate(description, description_width),
                theme::panel(),
            ),
        ]);
    }

    let key_width = 13.min(width.saturating_sub(2)).max(1);
    let description_width = width.saturating_sub(key_width + 1).max(1);
    Line::from(vec![
        Span::styled(
            pad_or_truncate(key, key_width),
            theme::muted().add_modifier(Modifier::BOLD),
        ),
        Span::raw(" "),
        Span::styled(
            pad_or_truncate(description, description_width),
            theme::panel(),
        ),
    ])
}

fn help_command_lines(commands: &[CommandSpec], width: usize) -> Vec<Line<'static>> {
    let mut lines = vec![Line::from(Span::styled("Slash commands", theme::accent()))];
    for (category, specs) in command_groups(commands) {
        for (idx, spec) in specs.into_iter().enumerate() {
            lines.push(help_command_line(
                width,
                if idx == 0 { category } else { "" },
                spec,
            ));
        }
    }
    lines
}

fn command_groups<'a>(commands: &'a [CommandSpec]) -> Vec<(&'static str, Vec<&'a CommandSpec>)> {
    let mut groups: Vec<(&'static str, Vec<&'a CommandSpec>)> = Vec::new();
    for spec in commands {
        if let Some((_, specs)) = groups
            .iter_mut()
            .find(|(category, _)| *category == spec.category)
        {
            specs.push(spec);
        } else {
            groups.push((spec.category, vec![spec]));
        }
    }
    groups
}

fn help_command_line(width: usize, category: &'static str, spec: &CommandSpec) -> Line<'static> {
    let (category_width, command_width, args_width, shortcut_width): (usize, usize, usize, usize) =
        if width >= 64 {
            (11, 14, 18, 3)
        } else if width >= 52 {
            (9, 13, 12, 3)
        } else {
            (0, 13, 14, 0)
        };
    let separator_count = usize::from(category_width > 0)
        + 1
        + usize::from(args_width > 0)
        + usize::from(shortcut_width > 0);
    let used = category_width
        .saturating_add(command_width)
        .saturating_add(args_width)
        .saturating_add(shortcut_width)
        .saturating_add(separator_count);
    let description_width = width.saturating_sub(used).max(1);
    let mut spans = Vec::new();

    if category_width > 0 {
        spans.push(Span::styled(
            pad_or_truncate(category, category_width),
            help_group_style(category),
        ));
        spans.push(Span::raw(" "));
    }
    spans.push(Span::styled(
        pad_or_truncate(&format!("/{}", spec.name), command_width),
        theme::title(),
    ));
    spans.push(Span::raw(" "));
    if args_width > 0 {
        spans.push(Span::styled(
            pad_or_truncate(spec.args, args_width),
            theme::muted(),
        ));
        spans.push(Span::raw(" "));
    }
    if shortcut_width > 0 {
        spans.push(Span::styled(
            pad_or_truncate(spec.shortcut.unwrap_or_default(), shortcut_width),
            theme::muted().add_modifier(Modifier::BOLD),
        ));
        spans.push(Span::raw(" "));
    }
    spans.push(Span::styled(
        pad_or_truncate(spec.description, description_width),
        theme::panel(),
    ));
    Line::from(spans)
}

fn help_group_style(value: &str) -> Style {
    if value.is_empty() {
        theme::muted()
    } else {
        theme::accent()
    }
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

pub(crate) fn draw_comment_menu(frame: &mut Frame, area: Rect, ui: &mut UiState) {
    if ui.comment_delete.is_some() {
        return;
    }
    let Some(menu) = ui.comment_menu else {
        return;
    };
    if area.width < 12 || area.height < 4 {
        return;
    }

    ui.hit_map.push(area, HitTarget::CommentMenuBackdrop);
    let width = 14.min(area.width);
    let height = 4.min(area.height);
    let max_x = area.x.saturating_add(area.width.saturating_sub(width));
    let max_y = area.y.saturating_add(area.height.saturating_sub(height));
    let rect = Rect::new(
        menu.x.clamp(area.x, max_x),
        menu.y.clamp(area.y, max_y),
        width,
        height,
    );
    let rows = [
        ("Edit", HitTarget::CommentMenuEdit(menu.target)),
        ("Delete", HitTarget::CommentMenuDelete(menu.target)),
    ];

    frame.render_widget(Clear, rect);
    frame.render_widget(
        List::new(
            rows.iter()
                .map(|(label, _)| ListItem::new(Line::from(*label)))
                .collect::<Vec<_>>(),
        )
        .style(theme::panel())
        .block(panel(" Message ", true)),
        rect,
    );

    let row_area = Rect::new(
        rect.x.saturating_add(1),
        rect.y.saturating_add(1),
        rect.width.saturating_sub(2),
        rect.height.saturating_sub(2),
    );
    for (idx, (_, target)) in rows.into_iter().enumerate() {
        if idx as u16 >= row_area.height {
            break;
        }
        ui.hit_map.push(
            Rect::new(row_area.x, row_area.y + idx as u16, row_area.width, 1),
            target,
        );
    }
}

pub(crate) fn draw_comment_delete_confirm(frame: &mut Frame, area: Rect, ui: &mut UiState) {
    let Some(confirm) = ui.comment_delete else {
        return;
    };
    if area.width < 24 || area.height < 5 {
        return;
    }

    ui.hit_map.push(area, HitTarget::CommentDeleteCancel);
    let modal = centered(area, 44, 7);
    let noun = confirm.target.noun();
    let index = confirm.target.index();
    let text = format!("Delete {noun} #{index}?  y / n");
    frame.render_widget(Clear, modal);
    frame.render_widget(
        Paragraph::new(vec![
            Line::from(text.clone()),
            Line::from(""),
            Line::from(Span::styled("This cannot be undone.", theme::muted())),
        ])
        .alignment(Alignment::Center)
        .style(theme::panel())
        .block(panel(&format!(" Delete {noun} "), true).padding(Padding::uniform(1))),
        modal,
    );

    let text_x = modal.x + modal.width.saturating_sub(text.chars().count() as u16) / 2;
    let text_y = modal.y.saturating_add(2);
    if let Some(y_pos) = text.find('y') {
        ui.hit_map.push(
            Rect::new(text_x + y_pos as u16, text_y, 1, 1),
            HitTarget::CommentDeleteConfirm(confirm.target),
        );
    }
    if let Some(n_pos) = text.rfind('n') {
        ui.hit_map.push(
            Rect::new(text_x + n_pos as u16, text_y, 1, 1),
            HitTarget::CommentDeleteCancel,
        );
    }
}

pub(crate) fn draw_banner(frame: &mut Frame, area: Rect, ui: &mut UiState) {
    let Some(banner) = ui.banner.as_ref().filter(|banner| banner.active()).cloned() else {
        return;
    };
    if banner.presentation == BannerPresentation::ListModal {
        if let Some(list) = banner.list.as_ref() {
            draw_list_modal(frame, area, list, ui);
        }
        return;
    }
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
                    theme::accent(),
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

pub(crate) fn draw_list_modal(frame: &mut Frame, area: Rect, list: &ListModal, ui: &mut UiState) {
    let bottom_bar_top = area
        .y
        .saturating_add(area.height)
        .saturating_sub(bottombar_height(ui));
    let available_height = bottom_bar_top.saturating_sub(area.y).saturating_sub(2);
    if area.width < 16 || available_height < 5 {
        return;
    }

    let desired_width = list_modal_width(list).saturating_add(4);
    let desired_height = list.rows.len().saturating_add(5).min(u16::MAX as usize) as u16;
    let modal = centered(
        Rect::new(area.x, area.y, area.width, available_height),
        desired_width,
        desired_height.clamp(5, available_height),
    );
    let content_width = modal.width.saturating_sub(4) as usize;
    let mut widths = list_column_widths(list, content_width);
    if widths.is_empty() {
        widths.push(content_width.max(1));
    }

    ui.hit_map.push(modal, HitTarget::BannerModal);
    let mut lines = Vec::new();
    if list.rows.is_empty() {
        lines.push(Line::from(Span::styled(list.empty.clone(), theme::muted())));
    } else {
        lines.push(list_modal_line(&list.columns, &widths, true));
        let visible_rows = modal.height.saturating_sub(5) as usize;
        for (idx, row) in list.rows.iter().take(visible_rows).enumerate() {
            lines.push(list_modal_line(row, &widths, false));
            if list.row_actions.get(idx).is_some_and(Option::is_some) {
                let y = modal.y.saturating_add(2).saturating_add(idx as u16);
                ui.hit_map.push(
                    Rect::new(
                        modal.x.saturating_add(2),
                        y,
                        modal.width.saturating_sub(4),
                        1,
                    ),
                    HitTarget::ListModalRow(idx),
                );
            }
        }
        if list.rows.len() > visible_rows {
            lines.push(Line::from(Span::styled(
                format!("{} more rows", list.rows.len() - visible_rows),
                theme::muted(),
            )));
        }
    }

    frame.render_widget(Clear, modal);
    frame.render_widget(
        Paragraph::new(lines)
            .style(theme::panel())
            .block(panel(&format!(" {} ", list.title), true).padding(Padding::uniform(1))),
        modal,
    );
}

fn list_modal_width(list: &ListModal) -> u16 {
    let width = list_column_widths(list, usize::MAX / 4)
        .into_iter()
        .sum::<usize>()
        .saturating_add(list.columns.len().saturating_sub(1) * 2)
        .max(list.empty.chars().count());
    width.min(u16::MAX as usize) as u16
}

fn list_column_widths(list: &ListModal, max_width: usize) -> Vec<usize> {
    let column_count = list
        .columns
        .len()
        .max(list.rows.iter().map(Vec::len).max().unwrap_or(0));
    if column_count == 0 {
        return Vec::new();
    }
    let mut widths = (0..column_count)
        .map(|idx| {
            let header_width = list
                .columns
                .get(idx)
                .map(|value| value.chars().count())
                .unwrap_or_default();
            let row_width = list
                .rows
                .iter()
                .filter_map(|row| row.get(idx))
                .map(|value| value.chars().count())
                .max()
                .unwrap_or_default();
            header_width.max(row_width).clamp(3, 28)
        })
        .collect::<Vec<_>>();
    let separators = column_count.saturating_sub(1) * 2;
    let mut total = widths.iter().sum::<usize>().saturating_add(separators);
    while total > max_width && widths.iter().any(|width| *width > 3) {
        if let Some((idx, _)) = widths.iter().enumerate().max_by_key(|(_, width)| **width) {
            widths[idx] = widths[idx].saturating_sub(1).max(3);
        }
        total = widths.iter().sum::<usize>().saturating_add(separators);
    }
    widths
}

fn list_modal_line(values: &[String], widths: &[usize], header: bool) -> Line<'static> {
    let mut spans = Vec::new();
    for (idx, width) in widths.iter().enumerate() {
        if idx > 0 {
            spans.push(Span::raw("  "));
        }
        let value = values.get(idx).cloned().unwrap_or_default();
        let text = pad_or_truncate(&value, *width);
        let style = if header {
            theme::muted().add_modifier(Modifier::BOLD)
        } else {
            list_cell_style(&value)
        };
        spans.push(Span::styled(text, style));
    }
    Line::from(spans)
}

fn list_cell_style(value: &str) -> Style {
    match value {
        "open" | "active" | "enabled" | "joined" | "read" | "accepted" => theme::accent(),
        "pending" | "joinable" | "unread" => theme::unread(),
        "revoked" | "disabled" | "archived" => Style::default().fg(theme::ERROR).bg(theme::PANEL),
        _ => theme::panel(),
    }
}

fn pad_or_truncate(value: &str, width: usize) -> String {
    let char_count = value.chars().count();
    if char_count > width {
        let keep = width.saturating_sub(1);
        let mut out = value.chars().take(keep).collect::<String>();
        out.push('~');
        return out;
    }
    format!("{value:<width$}")
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
