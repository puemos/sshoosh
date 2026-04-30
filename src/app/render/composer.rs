use super::*;
pub(crate) fn draw_bottombar(
    frame: &mut Frame,
    area: Rect,
    account: &Account,
    snapshot: &Snapshot,
    ui: &mut UiState,
) {
    frame.render_widget(Block::default().style(theme::base()), area);
    if area.height == 0 || area.width == 0 {
        return;
    }

    let card = Rect::new(area.x, area.y, area.width, area.height);
    frame.render_widget(Block::default().style(theme::composer()), card);
    if card.height == 0 || card.width == 0 {
        return;
    }

    let mode_color = if ui.mode == UiMode::Normal {
        theme::ACCENT
    } else {
        theme::WARN
    };

    let top_padding: u16 = if card.height >= 3 { 1 } else { 0 };
    let status_reserved: u16 = if card.height >= 3 { 2 } else { 1 };
    let input_height = card
        .height
        .saturating_sub(status_reserved.saturating_add(top_padding))
        .max(1);
    let input = Rect::new(
        card.x.saturating_add(2),
        card.y.saturating_add(top_padding),
        card.width.saturating_sub(4),
        input_height,
    );
    let cursor = if ui.mode == UiMode::Compose || ui.composer.buffer.is_empty() {
        "▌"
    } else {
        ""
    };
    let show_placeholder = ui.mode == UiMode::Normal && ui.composer.buffer.is_empty();
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
    let line = if show_placeholder {
        Line::from(vec![
            Span::styled(prompt, theme::composer()),
            Span::styled(
                "  Press / for a command, Enter to write…",
                theme::composer().fg(theme::MUTED),
            ),
        ])
    } else {
        Line::from(Span::styled(prompt, theme::composer()))
    };
    frame.render_widget(
        Paragraph::new(line)
            .style(theme::composer())
            .scroll((scroll_y, 0))
            .wrap(Wrap { trim: false }),
        input,
    );

    let status = Rect::new(
        card.x.saturating_add(2),
        card.y + card.height.saturating_sub(1),
        card.width.saturating_sub(4),
        1,
    );
    let keybar_width = keybar_width(ui);
    let keybar_start = status.x + status.width.saturating_sub(keybar_width);
    if keybar_width > 0 {
        frame.render_widget(
            Paragraph::new(keybar_line(ui)).style(theme::composer()),
            Rect::new(keybar_start, status.y, keybar_width.min(status.width), 1),
        );
    }
    let status_left_width = status
        .width
        .saturating_sub(keybar_width.saturating_add(2))
        .min(96);
    if status_left_width > 0 {
        let status_left = Rect::new(status.x, status.y, status_left_width, 1);
        draw_status_cluster(frame, status_left, account, snapshot, ui, mode_color);
    }
    register_keybar_actions(ui, status, keybar_start);
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

fn keybar_items(ui: &UiState) -> &'static [(&'static str, &'static str, Option<BottomBarAction>)] {
    match ui.mode {
        UiMode::Normal => &[
            ("tab", "detail", Some(BottomBarAction::ToggleDetail)),
            ("/", "command", Some(BottomBarAction::OpenCommand)),
            ("?", "help", Some(BottomBarAction::OpenHelp)),
            ("q", "quit", Some(BottomBarAction::OpenQuit)),
        ],
        UiMode::Compose => &[
            ("enter", "send", Some(BottomBarAction::SubmitComposer)),
            ("shift-enter", "newline", None),
            ("tab", "accept", Some(BottomBarAction::AcceptAutocomplete)),
            ("esc", "normal", Some(BottomBarAction::CloseMode)),
        ],
        UiMode::Palette => &[
            ("enter", "run", Some(BottomBarAction::RunPalette)),
            ("esc", "close", Some(BottomBarAction::CloseMode)),
        ],
        UiMode::Prompt => &[
            ("enter", "run", Some(BottomBarAction::RunPrompt)),
            ("esc", "close", Some(BottomBarAction::CloseMode)),
        ],
        UiMode::Help => &[("esc", "close", Some(BottomBarAction::CloseMode))],
        UiMode::ConfirmQuit => &[
            ("y", "quit", Some(BottomBarAction::ConfirmQuit)),
            ("n", "cancel", Some(BottomBarAction::CancelQuit)),
        ],
    }
}

fn keybar_width(ui: &UiState) -> u16 {
    keybar_items(ui)
        .iter()
        .enumerate()
        .map(|(idx, (key, label, _))| {
            let gap = if idx == 0 { 0 } else { 2 };
            gap + key.chars().count() + 2 + 1 + label.chars().count()
        })
        .sum::<usize>() as u16
}

fn keybar_line(ui: &UiState) -> Line<'static> {
    let mut spans = Vec::new();
    for (idx, (key, label, _)) in keybar_items(ui).iter().enumerate() {
        if idx > 0 {
            spans.push(Span::styled("  ", theme::composer()));
        }
        spans.push(Span::styled(
            format!(" {key} "),
            Style::default()
                .fg(theme::TEXT)
                .bg(theme::KEYCAP)
                .add_modifier(Modifier::BOLD),
        ));
        spans.push(Span::styled(" ", theme::composer()));
        spans.push(Span::styled(*label, theme::composer().fg(theme::MUTED)));
    }
    Line::from(spans)
}

pub(crate) fn register_keybar_actions(ui: &mut UiState, status: Rect, keybar_start: u16) {
    let mut cursor = keybar_start;
    for (idx, (key, label, action)) in keybar_items(ui).iter().enumerate() {
        if idx > 0 {
            cursor = cursor.saturating_add(2);
        }
        let width = key.chars().count() as u16 + 2 + 1 + label.chars().count() as u16;
        if let Some(action) = action
            && cursor < status.x.saturating_add(status.width)
        {
            let visible_width = width.min(status.x.saturating_add(status.width) - cursor);
            ui.hit_map.push(
                Rect::new(cursor, status.y, visible_width, 1),
                HitTarget::BottomBar(*action),
            );
        }
        cursor = cursor.saturating_add(width);
    }
}

fn draw_status_cluster(
    frame: &mut Frame,
    area: Rect,
    account: &Account,
    snapshot: &Snapshot,
    ui: &mut UiState,
    mode_color: ratatui::style::Color,
) {
    if area.is_empty() {
        return;
    }
    let active = active_label(snapshot, ui);
    let unread = snapshot.total_unread();
    let notifications = snapshot.notification_unread_count;
    let mentions = snapshot.mention_unread_count;
    let compact_width = char_width(mode_label(ui)).saturating_add(char_width(&active)) + 5;
    let show_badges = area.width as usize >= compact_width.saturating_add(38);
    let show_account = area.width as usize >= compact_width.saturating_add(62);

    let mut spans = Vec::new();
    let mode_pill_text = mode_label(ui).to_uppercase();
    spans.push(Span::styled(
        format!(" {mode_pill_text} "),
        Style::default()
            .fg(theme::BG)
            .bg(mode_color)
            .add_modifier(Modifier::BOLD),
    ));
    spans.push(Span::styled(" · ", theme::composer().fg(theme::MUTED)));
    spans.push(Span::styled(active, theme::composer().fg(theme::SUBTLE)));
    if show_badges {
        spans.push(Span::styled(" ", theme::composer()));
        push_badge(
            &mut spans,
            format!("{unread} unread"),
            unread > 0,
            theme::WARN,
        );
        let notification_start = spans_width(&spans) as u16;
        push_badge(
            &mut spans,
            format!("{notifications} notifications"),
            notifications > 0,
            theme::ACCENT_SOFT,
        );
        ui.hit_map.push(
            Rect::new(
                area.x.saturating_add(notification_start),
                area.y,
                (notifications.to_string().chars().count() as u16).saturating_add(15),
                1,
            ),
            HitTarget::TopbarNotifications,
        );
        let mention_start = spans_width(&spans) as u16;
        push_badge(
            &mut spans,
            format!("{mentions} mentions"),
            mentions > 0,
            theme::MENTION,
        );
        ui.hit_map.push(
            Rect::new(
                area.x.saturating_add(mention_start),
                area.y,
                (mentions.to_string().chars().count() as u16).saturating_add(10),
                1,
            ),
            HitTarget::TopbarMentions,
        );
    }
    if show_account {
        spans.push(Span::styled(
            format!("  {} online", snapshot.online_user_count()),
            theme::composer().fg(theme::MUTED),
        ));
        spans.push(Span::styled(
            format!("  {}", account.username),
            theme::composer().fg(theme::MUTED),
        ));
        spans.push(Span::styled(
            format!(" ({})", account.role.as_str()),
            theme::composer().fg(theme::MUTED),
        ));
    }

    frame.render_widget(
        Paragraph::new(Line::from(spans)).style(theme::composer()),
        area,
    );
}

fn push_badge(
    spans: &mut Vec<Span<'static>>,
    label: String,
    active: bool,
    active_color: ratatui::style::Color,
) {
    spans.push(Span::styled(
        format!(" {label} "),
        Style::default()
            .fg(if active { active_color } else { theme::MUTED })
            .bg(theme::BADGE),
    ));
}

fn spans_width(spans: &[Span<'_>]) -> usize {
    spans.iter().map(|span| span.content.chars().count()).sum()
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
    let height = visible_count as u16 + 4;
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
            let selected = idx == ui.composer.autocomplete.selected;
            let style = if selected {
                theme::selection()
            } else {
                theme::elevated_panel()
            };
            let glyph = if selected { "▸ " } else { "  " };
            ListItem::new(Line::from(vec![
                Span::styled(glyph, style),
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
