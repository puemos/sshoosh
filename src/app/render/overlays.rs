use super::*;

pub(crate) fn draw_palette(frame: &mut Frame, full_area: Rect, area: Rect, ui: &mut UiState) {
    ui.hit_map.push(full_area, HitTarget::PaletteBackdrop);
    let inner = elevated_panel(frame, area, "Command palette");
    if inner.width == 0 || inner.height < 3 {
        return;
    }
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(1),
            Constraint::Length(1),
            Constraint::Min(1),
        ])
        .split(inner);
    frame.render_widget(
        Paragraph::new(format!("> {}", ui.palette.query)).style(theme::elevated_panel()),
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
                theme::elevated_panel()
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
    frame.render_widget(
        Paragraph::new("Results").style(theme::elevated_accent()),
        chunks[1],
    );
    frame.render_widget(List::new(items).style(theme::elevated_panel()), chunks[2]);
    let rows = chunks[2];
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
    let inner = elevated_panel(frame, area, &ui.prompt.title);
    let text = if ui.prompt.input.is_empty() {
        format!("{}{}", ui.prompt.prefix, ui.prompt.placeholder)
    } else {
        format!("{}{}", ui.prompt.prefix, ui.prompt.input)
    };
    frame.render_widget(Paragraph::new(text).style(theme::elevated_panel()), inner);
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
    let inner = elevated_panel(frame, area, "Help");
    if inner.width == 0 || inner.height == 0 {
        return;
    }

    let mut lines = help_keyboard_lines(inner.width as usize);
    lines.extend(help_command_lines(commands, inner.width as usize));
    ui.hit_map.push(inner, HitTarget::HelpScroll);
    render_help_scroll(frame, inner, lines, &mut ui.help_scroll);
}

pub(crate) fn help_modal_area(area: Rect) -> Rect {
    centered(area, 104, 30)
}

fn render_help_scroll(
    frame: &mut Frame,
    area: Rect,
    lines: Vec<Line<'static>>,
    state: &mut ScrollViewState,
) {
    let content_height = lines.len().max(1).min(u16::MAX as usize) as u16;
    let mut scroll_view = ScrollView::new(Size::new(area.width, content_height))
        .vertical_scrollbar_visibility(ScrollbarVisibility::Automatic)
        .horizontal_scrollbar_visibility(ScrollbarVisibility::Never);
    scroll_view.render_widget(
        List::new(lines.into_iter().map(ListItem::new).collect::<Vec<_>>())
            .style(theme::elevated_panel())
            .highlight_style(theme::elevated_panel()),
        Rect::new(0, 0, area.width, content_height),
    );
    frame.render_stateful_widget(scroll_view, area, state);
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
        ("", "q / Ctrl-C", "quit / disconnect"),
    ];
    let mut lines = vec![Line::from(Span::styled(
        "Keyboard",
        theme::elevated_accent(),
    ))];
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
                theme::elevated_muted().add_modifier(Modifier::BOLD),
            ),
            Span::raw(" "),
            Span::styled(
                pad_or_truncate(description, description_width),
                theme::elevated_panel(),
            ),
        ]);
    }

    let key_width = 13.min(width.saturating_sub(2)).max(1);
    let description_width = width.saturating_sub(key_width + 1).max(1);
    Line::from(vec![
        Span::styled(
            pad_or_truncate(key, key_width),
            theme::elevated_muted().add_modifier(Modifier::BOLD),
        ),
        Span::raw(" "),
        Span::styled(
            pad_or_truncate(description, description_width),
            theme::elevated_panel(),
        ),
    ])
}

fn help_command_lines(commands: &[CommandSpec], width: usize) -> Vec<Line<'static>> {
    let mut lines = vec![Line::from(Span::styled(
        "Slash commands",
        theme::elevated_accent(),
    ))];
    for (category, specs) in command_groups(commands) {
        lines.push(Line::from(Span::styled(category, theme::elevated_accent())));
        for spec in specs {
            lines.extend(help_command_rows(width, spec));
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

fn help_command_rows(width: usize, spec: &CommandSpec) -> Vec<Line<'static>> {
    let subcommands = subcommands_for(spec.name);
    if subcommands.is_empty() {
        return vec![help_command_line(
            width,
            command_usage(spec.name, spec.args),
            spec.shortcut,
            spec.description,
        )];
    }

    subcommands
        .iter()
        .map(|subcommand| help_subcommand_line(width, spec, subcommand))
        .collect()
}

fn help_subcommand_line(
    width: usize,
    spec: &CommandSpec,
    subcommand: &SubcommandSpec,
) -> Line<'static> {
    help_command_line(
        width,
        command_usage(
            &format!("{} {}", spec.name, subcommand.name),
            subcommand.args,
        ),
        spec.shortcut
            .filter(|_| subcommand.name == "new" || subcommand.name == "open"),
        subcommand.description,
    )
}

fn command_usage(name: &str, args: &str) -> String {
    if args.is_empty() {
        format!("/{name}")
    } else {
        format!("/{name} {args}")
    }
}

fn help_command_line(
    width: usize,
    command: String,
    shortcut: Option<&'static str>,
    description: &'static str,
) -> Line<'static> {
    let shortcut_width = if width >= 56 { 3 } else { 0 };
    let command_width = if width >= 72 {
        34
    } else if width >= 52 {
        27
    } else {
        18
    }
    .min(width.saturating_sub(shortcut_width + 2).max(1));
    let used = command_width + 1 + shortcut_width + usize::from(shortcut_width > 0);
    let description_width = width.saturating_sub(used).max(1);
    let mut spans = vec![
        Span::styled(
            pad_or_truncate(&command, command_width),
            theme::elevated_title(),
        ),
        Span::raw(" "),
    ];
    if shortcut_width > 0 {
        spans.push(Span::styled(
            pad_or_truncate(shortcut.unwrap_or_default(), shortcut_width),
            theme::elevated_muted().add_modifier(Modifier::BOLD),
        ));
        spans.push(Span::raw(" "));
    }
    spans.push(Span::styled(
        pad_or_truncate(description, description_width),
        theme::elevated_panel(),
    ));
    Line::from(spans)
}

fn help_group_style(value: &str) -> Style {
    if value.is_empty() {
        theme::elevated_muted()
    } else {
        theme::elevated_accent()
    }
}

pub(crate) fn draw_confirm_quit(frame: &mut Frame, full_area: Rect, area: Rect, ui: &mut UiState) {
    ui.hit_map.push(full_area, HitTarget::ConfirmQuitBackdrop);
    let inner = elevated_panel(frame, area, "Quit");
    frame.render_widget(
        Paragraph::new("Disconnect from sshoosh?  y / n")
            .alignment(Alignment::Center)
            .style(theme::elevated_panel()),
        inner,
    );
    let text = "Disconnect from sshoosh?  y / n";
    let text_x = inner.x + inner.width.saturating_sub(text.chars().count() as u16) / 2;
    let text_y = inner.y;
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
    if area.width < 14 || area.height < 5 {
        return;
    }

    ui.hit_map.push(area, HitTarget::CommentMenuBackdrop);
    let width = 16.min(area.width);
    let height = 5.min(area.height);
    let max_x = area
        .x
        .saturating_add(area.width.saturating_sub(width.saturating_add(1)));
    let max_y = area
        .y
        .saturating_add(area.height.saturating_sub(height.saturating_add(1)));
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

    let row_area = elevated_panel(frame, rect, "Message");
    frame.render_widget(
        List::new(
            rows.iter()
                .map(|(label, _)| {
                    ListItem::new(Line::from(Span::styled(
                        label.to_string(),
                        theme::elevated_panel(),
                    )))
                })
                .collect::<Vec<_>>(),
        )
        .style(theme::elevated_panel()),
        row_area,
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
    let inner = elevated_panel(frame, modal, &format!("Delete {noun}"));
    frame.render_widget(
        Paragraph::new(vec![
            Line::from(text.clone()),
            Line::from(""),
            Line::from(Span::styled(
                "This cannot be undone.",
                theme::elevated_muted(),
            )),
        ])
        .alignment(Alignment::Center)
        .style(theme::elevated_panel()),
        inner,
    );

    let text_x = inner.x + inner.width.saturating_sub(text.chars().count() as u16) / 2;
    let text_y = inner.y;
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

    let inner = elevated_panel(frame, rect, "");
    frame.render_widget(
        Paragraph::new(banner.text.clone())
            .style(
                Style::default()
                    .fg(color)
                    .bg(theme::ELEVATED_PANEL)
                    .add_modifier(Modifier::BOLD),
            )
            .wrap(Wrap { trim: true }),
        inner,
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
                    Style::default()
                        .fg(theme::OK)
                        .bg(theme::ELEVATED_PANEL)
                        .add_modifier(Modifier::BOLD),
                )),
                Line::from(""),
                Line::from(Span::styled(
                    "c copies, Enter or Esc closes",
                    theme::elevated_accent(),
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
    let inner = elevated_panel(frame, modal, title);
    frame.render_widget(
        Paragraph::new(lines)
            .alignment(Alignment::Center)
            .style(theme::elevated_panel())
            .wrap(Wrap { trim: true }),
        inner,
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
    let inner = elevated_panel(frame, modal, &list.title);
    let content_width = inner.width as usize;
    let mut widths = list_column_widths(list, content_width);
    if widths.is_empty() {
        widths.push(content_width.max(1));
    }

    ui.hit_map.push(modal, HitTarget::BannerModal);
    let mut lines = Vec::new();
    if list.rows.is_empty() {
        lines.push(Line::from(Span::styled(
            list.empty.clone(),
            theme::elevated_muted(),
        )));
    } else {
        lines.push(list_modal_line(&list.columns, &widths, true));
        let visible_rows = inner.height.saturating_sub(1) as usize;
        for (idx, row) in list.rows.iter().take(visible_rows).enumerate() {
            lines.push(list_modal_line(row, &widths, false));
            if list.row_actions.get(idx).is_some_and(Option::is_some) {
                let y = inner.y.saturating_add(1).saturating_add(idx as u16);
                ui.hit_map.push(
                    Rect::new(inner.x, y, inner.width, 1),
                    HitTarget::ListModalRow(idx),
                );
            }
        }
        if list.rows.len() > visible_rows {
            lines.push(Line::from(Span::styled(
                format!("{} more rows", list.rows.len() - visible_rows),
                theme::elevated_muted(),
            )));
        }
    }

    frame.render_widget(Paragraph::new(lines).style(theme::elevated_panel()), inner);
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
            theme::elevated_muted().add_modifier(Modifier::BOLD)
        } else {
            list_cell_style(&value)
        };
        spans.push(Span::styled(text, style));
    }
    Line::from(spans)
}

fn list_cell_style(value: &str) -> Style {
    match value {
        "open" | "active" | "enabled" | "joined" | "read" | "accepted" => theme::elevated_accent(),
        "pending" | "joinable" | "unread" => theme::elevated_unread(),
        "revoked" | "disabled" | "archived" => {
            Style::default().fg(theme::ERROR).bg(theme::ELEVATED_PANEL)
        }
        _ => theme::elevated_panel(),
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

pub(crate) fn elevated_panel(frame: &mut Frame, area: Rect, title: &str) -> Rect {
    elevated_surface(frame, area);
    let title = title.trim();
    if !title.is_empty() && area.width > 4 && area.height > 2 {
        let title_area = Rect::new(
            area.x.saturating_add(2),
            area.y.saturating_add(1),
            area.width.saturating_sub(4),
            1,
        );
        frame.render_widget(
            Paragraph::new(title.to_string()).style(theme::elevated_accent()),
            title_area,
        );
    }
    elevated_inner(area, !title.is_empty())
}

fn elevated_surface(frame: &mut Frame, area: Rect) {
    if area.width == 0 || area.height == 0 {
        return;
    }

    frame.render_widget(Clear, area);
    frame.render_widget(Block::default().style(theme::elevated_panel()), area);
}

fn elevated_inner(area: Rect, has_title: bool) -> Rect {
    let horizontal_padding = 2.min(area.width);
    let top_padding = if has_title { 2 } else { 1 }.min(area.height);
    let remaining_height = area.height.saturating_sub(top_padding);
    let bottom_padding = 1.min(remaining_height.saturating_sub(1));
    Rect::new(
        area.x.saturating_add(horizontal_padding),
        area.y.saturating_add(top_padding),
        area.width
            .saturating_sub(horizontal_padding.saturating_mul(2)),
        remaining_height.saturating_sub(bottom_padding),
    )
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
