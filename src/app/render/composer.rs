use super::*;
pub(crate) fn draw_bottombar(frame: &mut Frame, area: Rect, snapshot: &Snapshot, ui: &mut UiState) {
    frame.render_widget(Block::default().style(theme::base()), area);
    if area.height == 0 || area.width == 0 {
        return;
    }

    let separator = Rect::new(area.x, area.y, area.width, 1);
    let separator_color = bottom_separator_color(ui);
    draw_horizontal_divider(frame, separator, separator_color);

    let card = Rect::new(
        area.x.saturating_add(1),
        area.y.saturating_add(1),
        area.width.saturating_sub(2),
        area.height.saturating_sub(1),
    );
    frame.render_widget(Block::default().style(theme::composer()), card);
    if card.height == 0 || card.width == 0 {
        return;
    }

    let edge = Rect::new(card.x, card.y, 1, card.height);
    let edge_text = (0..card.height).map(|_| "│").collect::<Vec<_>>().join("\n");
    let mode_color = if ui.mode == UiMode::Normal {
        theme::BORDER
    } else {
        theme::WARN
    };
    frame.render_widget(
        Paragraph::new(edge_text).style(Style::default().fg(mode_color).bg(theme::COMPOSER)),
        edge,
    );

    let input_height = card.height.saturating_sub(3).max(1);
    let input = Rect::new(
        card.x.saturating_add(2),
        card.y
            .saturating_add(1)
            .min(card.y + card.height.saturating_sub(1)),
        card.width.saturating_sub(4),
        input_height,
    );
    let cursor = if ui.mode == UiMode::Compose || ui.composer.buffer.is_empty() {
        "▌"
    } else {
        ""
    };
    let mut prompt = ui.composer.buffer.clone();
    if !cursor.is_empty() {
        let cursor_index = ui.composer.cursor.min(prompt.len());
        let cursor_index = if prompt.is_char_boundary(cursor_index) {
            cursor_index
        } else {
            prompt.len()
        };
        prompt.insert_str(cursor_index, cursor);
    }
    if prompt.is_empty() {
        prompt.push_str(cursor);
    }
    let scroll_y = composer_cursor_line(&ui.composer.buffer, ui.composer.cursor)
        .saturating_add(1)
        .saturating_sub(input.height);
    ui.hit_map
        .push(input, HitTarget::ComposerInput { scroll_y });
    frame.render_widget(
        Paragraph::new(prompt)
            .style(theme::composer())
            .scroll((scroll_y, 0))
            .wrap(Wrap { trim: false }),
        input,
    );

    let status = Rect::new(
        card.x.saturating_add(2),
        card.y + card.height.saturating_sub(2),
        card.width.saturating_sub(4),
        1,
    );
    let keybar = keybar_text(ui);
    let keybar_start = status.x + status.width.saturating_sub(keybar.chars().count() as u16);
    frame.render_widget(
        Paragraph::new(keybar)
            .alignment(Alignment::Right)
            .style(Style::default().fg(theme::MUTED).bg(theme::COMPOSER)),
        status,
    );
    let keybar_width = keybar.chars().count() as u16;
    let status_left_width = status
        .width
        .saturating_sub(keybar_width.saturating_add(2))
        .min(40);
    if status_left_width > 0 {
        let status_left = Rect::new(status.x, status.y, status_left_width, 1);
        frame.render_widget(
            Paragraph::new(Line::from(vec![
                Span::styled(
                    mode_label(ui),
                    Style::default().fg(mode_color).bg(theme::COMPOSER),
                ),
                Span::styled(
                    format!(" · {}", active_label(snapshot, ui)),
                    Style::default().fg(theme::MUTED).bg(theme::COMPOSER),
                ),
            ]))
            .style(theme::composer()),
            status_left,
        );
    }
    register_keybar_actions(ui, status, keybar, keybar_start);
}

pub(crate) fn bottom_separator_color(_ui: &UiState) -> ratatui::style::Color {
    theme::BORDER
}

pub(crate) fn mode_label(ui: &UiState) -> &'static str {
    match ui.mode {
        UiMode::Compose => "compose",
        UiMode::Normal => "normal",
        UiMode::Palette => "palette",
        UiMode::Prompt => "prompt",
        UiMode::Help => "help",
        UiMode::ConfirmQuit => "quit?",
    }
}

pub(crate) fn keybar_text(ui: &UiState) -> &'static str {
    match ui.mode {
        UiMode::Normal => "tab detail  / command  ? help  q quit",
        UiMode::Compose => {
            "enter send  shift-enter newline  tab accept  ctrl-x e edit last  esc normal"
        }
        UiMode::Palette => "type filter  enter run  esc close",
        UiMode::Prompt => "enter run  esc close",
        UiMode::Help => "esc close",
        UiMode::ConfirmQuit => "y quit  n cancel",
    }
}

pub(crate) fn register_keybar_actions(
    ui: &mut UiState,
    status: Rect,
    keybar: &str,
    keybar_start: u16,
) {
    let actions: &[(&str, BottomBarAction)] = match ui.mode {
        UiMode::Normal => &[
            ("tab detail", BottomBarAction::ToggleDetail),
            ("/ command", BottomBarAction::OpenCommand),
            ("? help", BottomBarAction::OpenHelp),
            ("q quit", BottomBarAction::OpenQuit),
        ],
        UiMode::Compose => &[
            ("enter send", BottomBarAction::SubmitComposer),
            ("tab accept", BottomBarAction::AcceptAutocomplete),
            ("esc normal", BottomBarAction::CloseMode),
        ],
        UiMode::Palette => &[
            ("enter run", BottomBarAction::RunPalette),
            ("esc close", BottomBarAction::CloseMode),
        ],
        UiMode::Prompt => &[
            ("enter run", BottomBarAction::RunPrompt),
            ("esc close", BottomBarAction::CloseMode),
        ],
        UiMode::Help => &[("esc close", BottomBarAction::CloseMode)],
        UiMode::ConfirmQuit => &[
            ("y quit", BottomBarAction::ConfirmQuit),
            ("n cancel", BottomBarAction::CancelQuit),
        ],
    };
    for (label, action) in actions {
        let Some(start) = keybar.find(label) else {
            continue;
        };
        ui.hit_map.push(
            Rect::new(
                keybar_start.saturating_add(start as u16),
                status.y,
                label.chars().count() as u16,
                1,
            ),
            HitTarget::BottomBar(*action),
        );
    }
}

pub(crate) fn composer_cursor_line(buffer: &str, cursor: usize) -> u16 {
    buffer
        .char_indices()
        .take_while(|(idx, _)| *idx < cursor)
        .filter(|(_, ch)| *ch == '\n')
        .count() as u16
}

pub(crate) fn draw_autocomplete(frame: &mut Frame, composer_area: Rect, ui: &mut UiState) {
    let visible_count = ui.composer.autocomplete.items.len().min(8);
    let height = visible_count as u16 + 3;
    let visible_items = &ui.composer.autocomplete.items[..visible_count];
    let label_width = visible_items
        .iter()
        .map(|item| item.label.chars().count().saturating_add(2))
        .max()
        .unwrap_or(12)
        .max(12);
    let detail_width = visible_items
        .iter()
        .map(|item| item.detail.chars().count().saturating_add(2))
        .max()
        .unwrap_or(18)
        .max(18);
    let preview_width = visible_items
        .iter()
        .map(|item| item.preview.chars().count())
        .max()
        .unwrap_or(0);
    let preferred_width = label_width
        .saturating_add(detail_width)
        .saturating_add(preview_width)
        .saturating_add(2)
        .max(62)
        .min(u16::MAX as usize) as u16;
    let width = preferred_width.min(composer_area.width.saturating_sub(2));
    let y = composer_area.y.saturating_sub(height);
    let area = Rect::new(composer_area.x + 1, y, width, height);
    let items: Vec<ListItem> = visible_items
        .iter()
        .enumerate()
        .map(|(idx, item)| {
            let style = if idx == ui.composer.autocomplete.selected {
                theme::selection()
            } else {
                theme::elevated_panel()
            };
            ListItem::new(Line::from(vec![
                Span::styled(format!("{:<label_width$}", item.label), style),
                Span::styled(format!("{:<detail_width$}", item.detail), style),
                Span::styled(item.preview.clone(), style),
            ]))
        })
        .collect();
    let rows = elevated_panel(frame, area, "Commands");
    frame.render_widget(List::new(items).style(theme::elevated_panel()), rows);
    ui.hit_map.push(rows, HitTarget::AutocompleteScroll);
    for idx in 0..ui.composer.autocomplete.items.len().min(8) {
        ui.hit_map.push(
            Rect::new(rows.x, rows.y + idx as u16, rows.width, 1),
            HitTarget::AutocompleteRow(idx),
        );
    }
}
