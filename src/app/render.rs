use ratatui::{
    Frame,
    layout::{Alignment, Constraint, Direction, Layout, Position, Rect, Size},
    style::{Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Clear, List, ListItem, Padding, Paragraph, Wrap},
};
use time::{OffsetDateTime, macros::format_description};
use tui_scrollview::{ScrollView, ScrollViewState, ScrollbarVisibility};

use crate::service::{Account, Snapshot};

use super::{
    commands::CommandSpec,
    state::{ActivePane, BannerPresentation, BottomBarAction, HitTarget, Route, UiMode, UiState},
    theme,
};

const WORKSPACE_PANE_WIDTH: u16 = 38;

pub fn draw(
    frame: &mut Frame,
    account: &Account,
    snapshot: &Snapshot,
    ui: &mut UiState,
    commands: &[CommandSpec],
) {
    let area = frame.area();
    ui.hit_map.clear();
    frame.render_widget(Clear, area);
    if !account.activated {
        draw_onboarding(frame, area, account, ui);
        draw_banner(frame, area, ui);
        return;
    }

    let shell = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(1),
            Constraint::Length(1),
            Constraint::Min(6),
            Constraint::Length(bottombar_height(ui)),
        ])
        .split(area);
    draw_topbar(frame, shell[0], account, snapshot, ui);
    draw_horizontal_divider(frame, shell[1], theme::BORDER);
    draw_body(frame, shell[2], snapshot, ui);
    draw_bottombar(frame, shell[3], snapshot, ui);
    draw_pane_divider_intersections(frame, area, shell[1], shell[3], bottom_separator_color(ui));
    draw_banner(frame, area, ui);

    match ui.mode {
        UiMode::Palette => draw_palette(frame, area, centered(area, 72, 18), ui),
        UiMode::Prompt => draw_prompt(frame, area, centered(area, 58, 7), ui),
        UiMode::Help => draw_help(frame, area, centered(area, 76, 20), commands, ui),
        UiMode::ConfirmQuit => draw_confirm_quit(frame, area, centered(area, 42, 5), ui),
        UiMode::Compose if ui.composer.autocomplete.open => draw_autocomplete(frame, shell[3], ui),
        _ => {}
    }
}

fn bottombar_height(ui: &UiState) -> u16 {
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

fn draw_onboarding(frame: &mut Frame, area: Rect, account: &Account, ui: &mut UiState) {
    let modal = centered(area, 72, 13);
    let block = panel(" sshoosh setup ", true);
    let text = vec![
        Line::from(Span::styled(
            "This SSH key is not activated yet.",
            theme::unread(),
        )),
        Line::from(""),
        Line::from("Ask an owner/admin for a one-time invite code."),
        Line::from("Type the code and press Enter, or use: /join CODE username"),
        Line::from(""),
        Line::from(vec![
            Span::styled("Suggested username: ", theme::muted()),
            Span::styled(account.username.clone(), theme::accent()),
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

fn draw_topbar(
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

fn draw_horizontal_divider(frame: &mut Frame, area: Rect, color: ratatui::style::Color) {
    if area.height == 0 || area.width == 0 {
        return;
    }
    frame.render_widget(
        Paragraph::new("─".repeat(area.width as usize))
            .style(Style::default().fg(color).bg(theme::BG)),
        area,
    );
}

fn active_label(snapshot: &Snapshot, ui: &UiState) -> String {
    match &ui.route {
        Route::Channel(id) => snapshot
            .channels
            .iter()
            .find(|channel| &channel.id == id)
            .map(|channel| format!("#{}", channel.slug))
            .unwrap_or_else(|| "#channel".to_string()),
        Route::Dms => snapshot
            .selected_conversation_id
            .as_ref()
            .and_then(|id| snapshot.conversations.iter().find(|dm| &dm.id == id))
            .map(|dm| format!("@{}", dm.peer_username))
            .unwrap_or_else(|| "DMs".to_string()),
    }
}

fn draw_body(frame: &mut Frame, area: Rect, snapshot: &Snapshot, ui: &mut UiState) {
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

fn pane_divider_x(area: Rect) -> Option<u16> {
    (area.width >= 80).then(|| area.x.saturating_add(WORKSPACE_PANE_WIDTH))
}

fn draw_vertical_divider(frame: &mut Frame, area: Rect) {
    if area.is_empty() {
        return;
    }
    let divider = (0..area.height).map(|_| "│").collect::<Vec<_>>().join("\n");
    frame.render_widget(
        Paragraph::new(divider).style(Style::default().fg(theme::BORDER).bg(theme::BG)),
        area,
    );
}

fn draw_pane_divider_intersections(
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

fn draw_divider_cell(
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

fn pane_inner(area: Rect) -> Rect {
    Rect::new(
        area.x.saturating_add(1),
        area.y.saturating_add(1),
        area.width.saturating_sub(2),
        area.height.saturating_sub(2),
    )
}

fn draw_workspace(frame: &mut Frame, area: Rect, snapshot: &Snapshot, ui: &mut UiState) {
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
        let icon = if channel.visibility == "private" {
            "◆"
        } else {
            "#"
        };
        let unread = snapshot.channel_unread(&channel.id);
        let unread_badge = unread_badge(unread);
        let label = truncate_text(
            format!("{icon}{}", channel.slug),
            row_width.saturating_sub(unread_badge.len()),
        );
        if selected && (ui.active_pane == ActivePane::Rail || snapshot.selected_thread_id.is_none())
        {
            selected_y = Some(items.len() as u16);
        }
        items.push(ListItem::new(Line::from(vec![
            Span::styled(label, workspace_label_style(selected, unread)),
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
                for thread in &snapshot.threads {
                    let row = items.len() as u16;
                    if ui.active_pane != ActivePane::Rail
                        && snapshot.selected_thread_id.as_deref() == Some(thread.id.as_str())
                    {
                        selected_y = Some(items.len() as u16);
                    }
                    items.push(thread_item(snapshot, thread, row_width));
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
        let label = truncate_text(
            format!("@{}", dm.peer_username),
            row_width.saturating_sub(unread_badge.len()),
        );
        if selected {
            selected_y = Some(items.len() as u16);
        }
        items.push(ListItem::new(Line::from(vec![
            Span::styled(label, workspace_label_style(selected, dm.unread_count)),
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

fn thread_item<'a>(
    snapshot: &Snapshot,
    thread: &'a crate::service::ThreadItem,
    row_width: usize,
) -> ListItem<'a> {
    let selected = snapshot.selected_thread_id.as_deref() == Some(thread.id.as_str());
    let unread_badge = unread_badge(thread.unread_count);
    let title = truncate_text(
        &thread.title,
        row_width.saturating_sub(4 + unread_badge.len()),
    );
    ListItem::new(Line::from(vec![
        Span::raw("  ↳ "),
        Span::styled(title, workspace_label_style(selected, thread.unread_count)),
        Span::styled(unread_badge, theme::unread()),
    ]))
}

fn workspace_label_style(selected: bool, unread_count: i64) -> Style {
    if selected {
        theme::title()
    } else if unread_count > 0 {
        theme::unread()
    } else {
        theme::muted()
    }
}

fn unread_badge(count: i64) -> String {
    if count > 0 {
        format!(" [{count}]")
    } else {
        String::new()
    }
}

fn truncate_text(value: impl AsRef<str>, max_chars: usize) -> String {
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

fn draw_detail(frame: &mut Frame, area: Rect, snapshot: &Snapshot, ui: &mut UiState) {
    if matches!(ui.route, Route::Dms) {
        draw_dm_detail(frame, area, snapshot, ui);
        return;
    }
    frame.render_widget(Block::default().style(theme::panel()), area);
    let area = pane_inner(area);
    let selected = snapshot
        .selected_thread_id
        .as_ref()
        .and_then(|id| snapshot.threads.iter().find(|thread| &thread.id == id));
    let mut items: Vec<ListItem> = Vec::new();
    let message_width = message_content_width(area);
    let title = selected
        .map(|thread| thread.title.as_str())
        .unwrap_or("No thread selected");
    draw_detail_header(frame, area, title, ui);
    let messages_area = pane_scroll_area(area);
    if let Some(thread) = selected {
        items.push(ListItem::new(vec![
            Line::from(vec![Span::styled(
                format!(
                    "@{} · {} comments · {}",
                    thread.author,
                    thread.comment_count,
                    thread.last_activity_at.as_deref().unwrap_or("no activity")
                ),
                theme::muted(),
            )]),
            Line::from(""),
        ]));
        items.push(message_card(
            snapshot,
            &thread.author,
            Some(&thread.created_at),
            &thread.body,
            message_width,
        ));
        for (idx, comment) in snapshot.comments.iter().enumerate() {
            if idx == 0 {
                items.push(message_gap());
            }
            items.push(message_card(
                snapshot,
                &comment.author,
                Some(&comment.created_at),
                &comment.body,
                message_width,
            ));
            if idx + 1 < snapshot.comments.len() {
                items.push(message_gap());
            }
        }
    } else {
        items.push(ListItem::new(vec![
            Line::from("No thread selected."),
            Line::from(Span::styled(
                "Create one with /thread title",
                theme::muted(),
            )),
        ]));
    }
    ui.hit_map.push(messages_area, HitTarget::DetailScroll);
    render_scroll_items(frame, messages_area, items, &mut ui.detail_scroll);
}

fn draw_workspace_header(frame: &mut Frame, area: Rect, title: &str, ui: &UiState) {
    draw_pane_header(
        frame,
        area,
        title,
        theme::section_header(matches!(&ui.route, Route::Channel(_))),
    );
}

fn draw_detail_header(frame: &mut Frame, area: Rect, title: &str, ui: &UiState) {
    draw_pane_header(
        frame,
        area,
        title,
        if ui.active_pane == ActivePane::Detail {
            theme::title()
        } else {
            theme::muted()
        },
    );
}

fn draw_pane_header(frame: &mut Frame, area: Rect, title: &str, style: Style) {
    if area.height == 0 || area.width == 0 {
        return;
    }
    let header = Rect::new(area.x, area.y, area.width, 1);
    frame.render_widget(
        Paragraph::new(Line::from(Span::styled(title.to_string(), style))).style(theme::panel()),
        header,
    );
}

fn pane_scroll_area(area: Rect) -> Rect {
    let header_height = area.height.min(1);
    Rect::new(
        area.x,
        area.y.saturating_add(header_height),
        area.width,
        area.height.saturating_sub(header_height),
    )
}

fn ensure_scroll_row_visible(state: &mut ScrollViewState, row: Option<u16>, viewport_height: u16) {
    let Some(row) = row else {
        return;
    };
    if viewport_height == 0 {
        return;
    }
    let offset = state.offset();
    let bottom = offset.y.saturating_add(viewport_height);
    let next_y = if row < offset.y {
        row
    } else if row >= bottom {
        row.saturating_add(1).saturating_sub(viewport_height)
    } else {
        offset.y
    };
    if next_y != offset.y || offset.x != 0 {
        state.set_offset(Position { x: 0, y: next_y });
    }
}

fn register_scroll_hits(
    ui: &mut UiState,
    area: Rect,
    scroll_target: HitTarget,
    row_hits: Vec<(u16, HitTarget)>,
    offset_y: u16,
) {
    ui.hit_map.push(area, scroll_target);
    let bottom = offset_y.saturating_add(area.height);
    for (row, target) in row_hits {
        if row < offset_y || row >= bottom {
            continue;
        }
        ui.hit_map.push(
            Rect::new(area.x, area.y + row.saturating_sub(offset_y), area.width, 1),
            target,
        );
    }
}

fn render_scroll_items(
    frame: &mut Frame,
    area: Rect,
    items: Vec<ListItem>,
    state: &mut ScrollViewState,
) {
    if area.is_empty() {
        return;
    }
    let content_height = items
        .iter()
        .map(ListItem::height)
        .sum::<usize>()
        .max(1)
        .min(u16::MAX as usize) as u16;
    let mut scroll_view = ScrollView::new(Size::new(area.width, content_height))
        .vertical_scrollbar_visibility(ScrollbarVisibility::Automatic)
        .horizontal_scrollbar_visibility(ScrollbarVisibility::Never);
    scroll_view.render_widget(
        List::new(items)
            .style(theme::panel())
            .highlight_style(theme::panel()),
        Rect::new(0, 0, area.width, content_height),
    );
    frame.render_stateful_widget(scroll_view, area, state);
}

fn draw_dm_detail(frame: &mut Frame, area: Rect, snapshot: &Snapshot, ui: &mut UiState) {
    frame.render_widget(Block::default().style(theme::panel()), area);
    let area = pane_inner(area);
    let title = snapshot
        .selected_conversation_id
        .as_ref()
        .and_then(|id| snapshot.conversations.iter().find(|dm| &dm.id == id))
        .map(|dm| format!("DM @{}", dm.peer_username))
        .unwrap_or_else(|| "DMs".to_string());
    draw_detail_header(frame, area, &title, ui);
    let messages_area = pane_scroll_area(area);
    let message_width = message_content_width(area);
    let mut items: Vec<ListItem> = Vec::new();
    if snapshot.conversation_messages.is_empty() {
        items.push(ListItem::new(vec![
            Line::from("No messages yet."),
            Line::from(Span::styled(
                "Type a message or use /dm @user",
                theme::muted(),
            )),
        ]));
    } else {
        for (idx, message) in snapshot.conversation_messages.iter().enumerate() {
            if idx > 0 {
                items.push(message_gap());
            }
            items.push(message_card(
                snapshot,
                &message.author,
                Some(&message.created_at),
                &message.body,
                message_width,
            ));
        }
    }
    ui.hit_map.push(messages_area, HitTarget::DetailScroll);
    render_scroll_items(frame, messages_area, items, &mut ui.detail_scroll);
}

fn message_card<'a>(
    snapshot: &Snapshot,
    author: &str,
    created_at: Option<&str>,
    body: &str,
    width: usize,
) -> ListItem<'a> {
    let is_current_user = snapshot
        .current_username
        .as_deref()
        .is_some_and(|username| username.eq_ignore_ascii_case(author));
    let gutter = Style::default().fg(theme::BORDER).bg(theme::PANEL);
    let mut lines = Vec::new();

    for row in wrap_message_body(body, width) {
        lines.push(message_card_line(
            gutter,
            vec![Span::styled(row, theme::message_body())],
        ));
    }
    let mut meta = vec![Span::styled(
        format!("@{}", author),
        theme::message_author(is_current_user),
    )];
    if let Some(created_at) = created_at.and_then(format_message_created_at) {
        meta.push(Span::styled(
            format!(" · {created_at}"),
            theme::message_meta(),
        ));
    }
    lines.push(message_card_line(gutter, meta));

    ListItem::new(lines).style(theme::message_card())
}

fn format_message_created_at(created_at: &str) -> Option<String> {
    format_message_created_at_at(created_at, OffsetDateTime::now_utc())
}

fn format_message_created_at_at(created_at: &str, now: OffsetDateTime) -> Option<String> {
    let created_at =
        OffsetDateTime::parse(created_at, &time::format_description::well_known::Rfc3339).ok()?;
    let seconds = (now - created_at).whole_seconds().max(0);
    match seconds {
        0..=59 => Some("just now".to_string()),
        60..=3_599 => Some(format!("{}m ago", seconds / 60)),
        3_600..=86_399 => Some(format!("{}h ago", seconds / 3_600)),
        86_400..=604_799 => Some(format!("{}d ago", seconds / 86_400)),
        _ => created_at
            .format(format_description!(
                "[month repr:short] [day padding:none], [year] [hour]:[minute] UTC"
            ))
            .ok(),
    }
}

fn message_gap<'a>() -> ListItem<'a> {
    ListItem::new(Line::from("")).style(theme::panel())
}

fn message_card_line<'a>(gutter: Style, content: Vec<Span<'a>>) -> Line<'a> {
    let mut spans = vec![Span::styled("│ ", gutter)];
    spans.extend(content);
    Line::from(spans)
}

fn message_content_width(area: Rect) -> usize {
    area.width.saturating_sub(4).max(8) as usize
}

fn wrap_message_body(body: &str, width: usize) -> Vec<String> {
    let width = width.max(1);
    let mut wrapped = Vec::new();
    for raw in body.lines() {
        let mut line = String::new();
        for ch in raw.chars() {
            if line.chars().count() == width {
                wrapped.push(std::mem::take(&mut line));
            }
            line.push(ch);
        }
        wrapped.push(line);
    }

    if wrapped.is_empty() {
        wrapped.push(String::new());
    }
    wrapped
}

fn draw_bottombar(frame: &mut Frame, area: Rect, snapshot: &Snapshot, ui: &mut UiState) {
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

fn bottom_separator_color(_ui: &UiState) -> ratatui::style::Color {
    theme::BORDER
}

fn mode_label(ui: &UiState) -> &'static str {
    match ui.mode {
        UiMode::Compose => "compose",
        UiMode::Normal => "normal",
        UiMode::Palette => "palette",
        UiMode::Prompt => "prompt",
        UiMode::Help => "help",
        UiMode::ConfirmQuit => "quit?",
    }
}

fn keybar_text(ui: &UiState) -> &'static str {
    match ui.mode {
        UiMode::Normal => "tab detail  / command  ? help  q quit",
        UiMode::Compose => "enter send  shift-enter newline  tab accept  esc normal",
        UiMode::Palette => "type filter  enter run  esc close",
        UiMode::Prompt => "enter run  esc close",
        UiMode::Help => "esc close",
        UiMode::ConfirmQuit => "y quit  n cancel",
    }
}

fn register_keybar_actions(ui: &mut UiState, status: Rect, keybar: &str, keybar_start: u16) {
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

fn composer_cursor_line(buffer: &str, cursor: usize) -> u16 {
    buffer
        .char_indices()
        .take_while(|(idx, _)| *idx < cursor)
        .filter(|(_, ch)| *ch == '\n')
        .count() as u16
}

fn draw_autocomplete(frame: &mut Frame, composer_area: Rect, ui: &mut UiState) {
    let height = ui.composer.autocomplete.items.len().min(8) as u16 + 2;
    let width = 62.min(composer_area.width.saturating_sub(2));
    let y = composer_area.y.saturating_sub(height);
    let area = Rect::new(composer_area.x + 1, y, width, height);
    let items: Vec<ListItem> = ui
        .composer
        .autocomplete
        .items
        .iter()
        .enumerate()
        .map(|(idx, item)| {
            let style = if idx == ui.composer.autocomplete.selected {
                Style::default().fg(theme::BG).bg(theme::ACCENT)
            } else {
                theme::panel()
            };
            ListItem::new(Line::from(vec![
                Span::styled(format!("{:<12}", item.label), style),
                Span::styled(format!("{:<18}", item.detail), style),
                Span::styled(item.preview.clone(), style),
            ]))
        })
        .collect();
    frame.render_widget(Clear, area);
    frame.render_widget(List::new(items).block(panel(" Commands ", true)), area);
    let rows = Rect::new(
        area.x.saturating_add(1),
        area.y.saturating_add(1),
        area.width.saturating_sub(2),
        area.height.saturating_sub(2),
    );
    ui.hit_map.push(rows, HitTarget::AutocompleteScroll);
    for idx in 0..ui.composer.autocomplete.items.len().min(8) {
        ui.hit_map.push(
            Rect::new(rows.x, rows.y + idx as u16, rows.width, 1),
            HitTarget::AutocompleteRow(idx),
        );
    }
}

fn draw_palette(frame: &mut Frame, full_area: Rect, area: Rect, ui: &mut UiState) {
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
                Style::default().fg(theme::BG).bg(theme::ACCENT)
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

fn draw_prompt(frame: &mut Frame, full_area: Rect, area: Rect, ui: &mut UiState) {
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

fn draw_help(
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
        Line::from("/ compose command · Tab accepts suggestions · Space toggles threads"),
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

fn draw_confirm_quit(frame: &mut Frame, full_area: Rect, area: Rect, ui: &mut UiState) {
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

fn draw_banner(frame: &mut Frame, area: Rect, ui: &mut UiState) {
    let Some(banner) = ui.banner.as_ref().filter(|banner| banner.active()).cloned() else {
        return;
    };
    if banner.presentation == BannerPresentation::Modal {
        draw_banner_modal(frame, area, &banner, ui);
        return;
    }

    let width = area.width.saturating_sub(4).min(80);
    let rect = Rect::new(area.x + 2, area.y, width, 1);
    frame.render_widget(
        Paragraph::new(banner.text.clone()).style(
            Style::default()
                .fg(if banner.error {
                    theme::ERROR
                } else {
                    theme::OK
                })
                .bg(theme::BG)
                .add_modifier(Modifier::BOLD),
        ),
        rect,
    );
}

fn draw_banner_modal(
    frame: &mut Frame,
    area: Rect,
    banner: &super::state::Banner,
    ui: &mut UiState,
) {
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
                Line::from(Span::styled("Enter or Esc closes", theme::muted())),
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

fn panel(title: &str, active: bool) -> Block<'_> {
    Block::default()
        .title(title.to_string())
        .borders(Borders::ALL)
        .border_style(theme::border(active))
        .style(theme::panel())
}

fn centered(area: Rect, width: u16, height: u16) -> Rect {
    let width = width.min(area.width.saturating_sub(2)).max(1);
    let height = height.min(area.height.saturating_sub(2)).max(1);
    Rect::new(
        area.x + area.width.saturating_sub(width) / 2,
        area.y + area.height.saturating_sub(height) / 2,
        width,
        height,
    )
}

#[cfg(test)]
mod tests {
    use ratatui::{
        Terminal,
        backend::TestBackend,
        buffer::{Buffer, Cell},
    };

    use crate::service::{
        Channel, CommentItem, Conversation, ConversationMessage, Role, ThreadItem,
    };

    use super::*;

    #[test]
    fn message_created_at_uses_relative_then_absolute_labels() {
        let now = OffsetDateTime::parse(
            "2026-04-30T12:00:00Z",
            &time::format_description::well_known::Rfc3339,
        )
        .unwrap();

        assert_eq!(
            format_message_created_at_at("2026-04-30T11:59:35Z", now).as_deref(),
            Some("just now")
        );
        assert_eq!(
            format_message_created_at_at("2026-04-30T11:55:00Z", now).as_deref(),
            Some("5m ago")
        );
        assert_eq!(
            format_message_created_at_at("2026-04-30T09:00:00Z", now).as_deref(),
            Some("3h ago")
        );
        assert_eq!(
            format_message_created_at_at("2026-04-20T09:08:00Z", now).as_deref(),
            Some("Apr 20, 2026 09:08 UTC")
        );
    }

    #[test]
    fn render_empty_main_at_common_sizes() {
        for (width, height) in [(80, 24), (100, 32), (140, 40)] {
            let backend = TestBackend::new(width, height);
            let mut terminal = Terminal::new(backend).unwrap();
            let account = Account {
                id: "a".to_string(),
                username: "owner".to_string(),
                display_name: "Owner".to_string(),
                role: Role::Owner,
                activated: true,
            };
            let mut ui = UiState::default();
            terminal
                .draw(|frame| draw(frame, &account, &Snapshot::default(), &mut ui, &[]))
                .unwrap();
            let buffer = terminal.backend().buffer();
            assert!(format!("{buffer:?}").contains("sshoosh"));
        }
    }

    #[test]
    fn topbar_and_pane_headers_use_compact_aligned_layout() {
        let backend = TestBackend::new(80, 24);
        let mut terminal = Terminal::new(backend).unwrap();
        let account = Account {
            id: "a".to_string(),
            username: "owner".to_string(),
            display_name: "Owner".to_string(),
            role: Role::Owner,
            activated: true,
        };
        let snapshot = Snapshot {
            channels: vec![Channel {
                id: "general".to_string(),
                slug: "general".to_string(),
                name: "general".to_string(),
                visibility: "public".to_string(),
                topic: None,
                unread_count: 1,
            }],
            threads: vec![ThreadItem {
                id: "thread".to_string(),
                channel_id: "general".to_string(),
                title: "wow".to_string(),
                body: "Body".to_string(),
                author: "owner".to_string(),
                comment_count: 0,
                last_comment_index: 0,
                unread_count: 0,
                last_activity_at: Some("now".to_string()),
                created_at: "2026-04-30T00:00:00Z".to_string(),
            }],
            selected_channel_id: Some("general".to_string()),
            selected_thread_id: Some("thread".to_string()),
            ..Snapshot::default()
        };
        let mut ui = UiState::default();
        ui.route = Route::Channel("general".to_string());
        ui.active_pane = ActivePane::Detail;

        terminal
            .draw(|frame| draw(frame, &account, &snapshot, &mut ui, &[]))
            .unwrap();
        let buffer = terminal.backend().buffer();

        assert_eq!(buffer.cell((9, 0)).expect("active label").symbol(), "#");
        assert_eq!(buffer.cell((0, 1)).expect("top divider").symbol(), "─");
        assert_eq!(buffer.cell((38, 1)).expect("top connector").symbol(), "┬");
        assert_eq!(buffer.cell((79, 1)).expect("top divider").symbol(), "─");
        assert_eq!(buffer.cell((38, 2)).expect("pane divider").symbol(), "│");
        assert_eq!(buffer.cell((38, 18)).expect("pane divider").symbol(), "│");
        assert_eq!(buffer.cell((0, 19)).expect("bottom divider").symbol(), "─");
        assert_eq!(
            buffer.cell((38, 19)).expect("bottom connector").symbol(),
            "┴"
        );
        assert_eq!(buffer.cell((79, 19)).expect("bottom divider").symbol(), "─");
        assert_eq!(buffer.cell((1, 3)).expect("workspace header").symbol(), "C");
        assert_eq!(buffer.cell((40, 3)).expect("detail header").symbol(), "w");
    }

    #[test]
    fn invite_code_uses_modal_without_covering_topbar() {
        let backend = TestBackend::new(100, 30);
        let mut terminal = Terminal::new(backend).unwrap();
        let account = Account {
            id: "a".to_string(),
            username: "owner".to_string(),
            display_name: "Owner".to_string(),
            role: Role::Owner,
            activated: true,
        };
        let mut ui = UiState::default();
        ui.banner = Some(super::super::state::Banner::modal_ok("Invite code: abc123"));

        terminal
            .draw(|frame| draw(frame, &account, &Snapshot::default(), &mut ui, &[]))
            .unwrap();
        let buffer = terminal.backend().buffer();
        let rendered = format!("{buffer:?}");
        assert!(rendered.contains("sshoosh"));
        assert!(rendered.contains("Invite code"));
        assert!(rendered.contains("abc123"));
    }

    #[test]
    fn workspace_section_headers_do_not_use_item_style() {
        let width = 80;
        let height = 24;
        let backend = TestBackend::new(width, height);
        let mut terminal = Terminal::new(backend).unwrap();
        let account = Account {
            id: "a".to_string(),
            username: "owner".to_string(),
            display_name: "Owner".to_string(),
            role: Role::Owner,
            activated: true,
        };
        let snapshot = Snapshot {
            channels: vec![
                Channel {
                    id: "general".to_string(),
                    slug: "general".to_string(),
                    name: "general".to_string(),
                    visibility: "public".to_string(),
                    topic: None,
                    unread_count: 0,
                },
                Channel {
                    id: "party".to_string(),
                    slug: "party".to_string(),
                    name: "party".to_string(),
                    visibility: "public".to_string(),
                    topic: None,
                    unread_count: 0,
                },
            ],
            conversations: vec![Conversation {
                id: "dm".to_string(),
                peer_username: "alice".to_string(),
                last_message_index: 1,
                unread_count: 0,
                last_activity_at: None,
                last_message_preview: None,
            }],
            selected_channel_id: Some("general".to_string()),
            ..Snapshot::default()
        };
        let mut ui = UiState::default();
        ui.route = Route::Channel("general".to_string());

        terminal
            .draw(|frame| draw(frame, &account, &snapshot, &mut ui, &[]))
            .unwrap();
        let buffer = terminal.backend().buffer();

        let channels = cell_for_text(buffer, width, height, "Channels");
        assert_eq!(channels.fg, theme::ACCENT);
        assert!(channels.modifier.contains(Modifier::BOLD));

        let dms = cell_for_text(buffer, width, height, "DMs");
        assert_eq!(dms.fg, theme::SUBTLE);
        assert!(dms.modifier.contains(Modifier::BOLD));

        let channel_item = cell_for_text(buffer, width, height, "#party");
        assert_eq!(channel_item.fg, theme::MUTED);
        assert!(!channel_item.modifier.contains(Modifier::BOLD));

        let dm_item = cell_for_text(buffer, width, height, "@alice");
        assert_eq!(dm_item.fg, theme::MUTED);
        assert!(!dm_item.modifier.contains(Modifier::BOLD));
    }

    #[test]
    fn workspace_thread_rows_are_single_line_and_truncated() {
        let backend = TestBackend::new(42, 16);
        let mut terminal = Terminal::new(backend).unwrap();
        let account = Account {
            id: "a".to_string(),
            username: "owner".to_string(),
            display_name: "Owner".to_string(),
            role: Role::Owner,
            activated: true,
        };
        let snapshot = Snapshot {
            channels: vec![Channel {
                id: "general".to_string(),
                slug: "general".to_string(),
                name: "general".to_string(),
                visibility: "public".to_string(),
                topic: None,
                unread_count: 0,
            }],
            threads: vec![ThreadItem {
                id: "thread".to_string(),
                channel_id: "general".to_string(),
                title: "A very long thread title that should be clipped".to_string(),
                body: "Body".to_string(),
                author: "owner".to_string(),
                comment_count: 3,
                last_comment_index: 3,
                unread_count: 0,
                last_activity_at: Some("2026-04-30T00:00:00Z".to_string()),
                created_at: "2026-04-30T00:00:00Z".to_string(),
            }],
            selected_channel_id: Some("general".to_string()),
            selected_thread_id: Some("thread".to_string()),
            ..Snapshot::default()
        };
        let mut ui = UiState::default();
        ui.route = Route::Channel("general".to_string());
        ui.active_pane = ActivePane::List;

        terminal
            .draw(|frame| draw(frame, &account, &snapshot, &mut ui, &[]))
            .unwrap();
        let buffer = terminal.backend().buffer();
        let rendered = format!("{buffer:?}");
        assert!(rendered.contains("A very long thread"));
        assert!(rendered.contains("..."));
        assert!(!rendered.contains("@owner"));
        assert!(!rendered.contains("3 comments"));
        assert!(!rendered.contains("2026-04-30"));
        assert!(!rendered.contains(">"));
        let channel_cell = buffer.cell((1, 4)).expect("channel cell");
        assert_eq!(channel_cell.symbol(), "#");
        assert_eq!(channel_cell.fg, theme::TEXT);
        assert!(channel_cell.modifier.contains(Modifier::BOLD));
    }

    #[test]
    fn render_dm_messages_with_scannable_rows() {
        let backend = TestBackend::new(100, 30);
        let mut terminal = Terminal::new(backend).unwrap();
        let account = Account {
            id: "a".to_string(),
            username: "owner".to_string(),
            display_name: "Owner".to_string(),
            role: Role::Owner,
            activated: true,
        };
        let snapshot = Snapshot {
            current_username: Some("owner".to_string()),
            conversations: vec![Conversation {
                id: "dm".to_string(),
                peer_username: "alice".to_string(),
                last_message_index: 2,
                unread_count: 0,
                last_activity_at: None,
                last_message_preview: Some("Hi Alice".to_string()),
            }],
            conversation_messages: vec![
                ConversationMessage {
                    author: "alice".to_string(),
                    obj_index: 1,
                    body: "Hello owner".to_string(),
                    created_at: "2020-01-02T03:04:00Z".to_string(),
                },
                ConversationMessage {
                    author: "owner".to_string(),
                    obj_index: 2,
                    body: "Hi Alice".to_string(),
                    created_at: "2020-01-02T03:05:00Z".to_string(),
                },
            ],
            selected_conversation_id: Some("dm".to_string()),
            ..Snapshot::default()
        };
        let mut ui = UiState::default();
        ui.route = Route::Dms;
        ui.active_pane = ActivePane::Detail;

        terminal
            .draw(|frame| draw(frame, &account, &snapshot, &mut ui, &[]))
            .unwrap();
        let rendered = format!("{:?}", terminal.backend().buffer());
        assert!(rendered.contains("@alice"));
        assert!(rendered.contains("@owner"));
        assert!(rendered.contains("Jan 2, 2020 03:04 UTC"));
        assert!(!rendered.contains(" you ·"));
        assert!(!rendered.contains("· #"));
        assert!(rendered.contains("Hello owner"));
        assert!(rendered.contains("Hi Alice"));
        assert!(!rendered.contains("●"));
        assert!(!rendered.contains("Replies"));
    }

    #[test]
    fn render_thread_detail_uses_thread_title_header() {
        let backend = TestBackend::new(100, 30);
        let mut terminal = Terminal::new(backend).unwrap();
        let account = Account {
            id: "a".to_string(),
            username: "owner".to_string(),
            display_name: "Owner".to_string(),
            role: Role::Owner,
            activated: true,
        };
        let snapshot = Snapshot {
            current_username: Some("owner".to_string()),
            threads: vec![ThreadItem {
                id: "thread".to_string(),
                channel_id: "channel".to_string(),
                title: "Deploy notes".to_string(),
                body: "Original post".to_string(),
                author: "owner".to_string(),
                comment_count: 1,
                last_comment_index: 2,
                unread_count: 0,
                last_activity_at: Some("now".to_string()),
                created_at: "2020-01-02T03:04:00Z".to_string(),
            }],
            comments: vec![CommentItem {
                id: "comment".to_string(),
                author: "alice".to_string(),
                obj_index: 2,
                body: "Looks good".to_string(),
                created_at: "2020-01-02T03:05:00Z".to_string(),
            }],
            selected_channel_id: Some("channel".to_string()),
            selected_thread_id: Some("thread".to_string()),
            ..Snapshot::default()
        };
        let mut ui = UiState::default();
        ui.route = Route::Channel("channel".to_string());
        ui.active_pane = ActivePane::Detail;

        terminal
            .draw(|frame| draw(frame, &account, &snapshot, &mut ui, &[]))
            .unwrap();
        let rendered = format!("{:?}", terminal.backend().buffer());
        assert!(rendered.contains("Deploy notes"));
        assert!(!rendered.contains("Detail"));
    }

    #[test]
    fn render_populates_hit_map_for_workspace_detail_and_composer() {
        let backend = TestBackend::new(80, 24);
        let mut terminal = Terminal::new(backend).unwrap();
        let account = Account {
            id: "a".to_string(),
            username: "owner".to_string(),
            display_name: "Owner".to_string(),
            role: Role::Owner,
            activated: true,
        };
        let snapshot = Snapshot {
            channels: vec![Channel {
                id: "general".to_string(),
                slug: "general".to_string(),
                name: "general".to_string(),
                visibility: "public".to_string(),
                topic: None,
                unread_count: 0,
            }],
            threads: vec![ThreadItem {
                id: "thread".to_string(),
                channel_id: "general".to_string(),
                title: "Deploy notes".to_string(),
                body: "Original post".to_string(),
                author: "owner".to_string(),
                comment_count: 0,
                last_comment_index: 1,
                unread_count: 0,
                last_activity_at: None,
                created_at: "2020-01-02T03:04:00Z".to_string(),
            }],
            selected_channel_id: Some("general".to_string()),
            selected_thread_id: Some("thread".to_string()),
            ..Snapshot::default()
        };
        let mut ui = UiState::default();
        ui.route = Route::Channel("general".to_string());

        terminal
            .draw(|frame| draw(frame, &account, &snapshot, &mut ui, &[]))
            .unwrap();

        assert!(matches!(
            ui.hit_map.hit(1, 4).map(|region| region.target),
            Some(HitTarget::WorkspaceChannel(id)) if id == "general"
        ));
        assert!(matches!(
            ui.hit_map.hit(1, 5).map(|region| region.target),
            Some(HitTarget::WorkspaceThread(id)) if id == "thread"
        ));
        assert!(matches!(
            ui.hit_map.hit(40, 4).map(|region| region.target),
            Some(HitTarget::DetailScroll)
        ));
        assert!(matches!(
            ui.hit_map.hit(3, 21).map(|region| region.target),
            Some(HitTarget::ComposerInput { .. })
        ));
    }

    fn cell_for_text<'a>(buffer: &'a Buffer, width: u16, height: u16, text: &str) -> &'a Cell {
        for y in 0..height {
            let mut row = String::new();
            for x in 0..width {
                row.push_str(buffer.cell((x, y)).expect("cell").symbol());
            }
            if let Some(x) = row.find(text) {
                return buffer.cell((x as u16, y)).expect("cell");
            }
        }
        panic!("could not find {text:?}");
    }

    #[test]
    fn render_dm_detail_uses_scroll_offset_for_messages() {
        let backend = TestBackend::new(100, 16);
        let mut terminal = Terminal::new(backend).unwrap();
        let account = Account {
            id: "a".to_string(),
            username: "owner".to_string(),
            display_name: "Owner".to_string(),
            role: Role::Owner,
            activated: true,
        };
        let snapshot = Snapshot {
            current_username: Some("owner".to_string()),
            conversations: vec![Conversation {
                id: "dm".to_string(),
                peer_username: "alice".to_string(),
                last_message_index: 3,
                unread_count: 0,
                last_activity_at: None,
                last_message_preview: None,
            }],
            conversation_messages: vec![
                ConversationMessage {
                    author: "alice".to_string(),
                    obj_index: 1,
                    body: "First message".to_string(),
                    created_at: "2020-01-02T03:04:00Z".to_string(),
                },
                ConversationMessage {
                    author: "owner".to_string(),
                    obj_index: 2,
                    body: "Second message".to_string(),
                    created_at: "2020-01-02T03:05:00Z".to_string(),
                },
                ConversationMessage {
                    author: "alice".to_string(),
                    obj_index: 3,
                    body: "Third message".to_string(),
                    created_at: "2020-01-02T03:06:00Z".to_string(),
                },
            ],
            selected_conversation_id: Some("dm".to_string()),
            ..Snapshot::default()
        };
        let mut ui = UiState::default();
        ui.route = Route::Dms;
        ui.active_pane = ActivePane::Detail;
        ui.detail_scroll
            .set_offset(ratatui::layout::Position { x: 0, y: 2 });

        terminal
            .draw(|frame| draw(frame, &account, &snapshot, &mut ui, &[]))
            .unwrap();
        let rendered = format!("{:?}", terminal.backend().buffer());
        assert!(!rendered.contains("First message"));
        assert!(rendered.contains("Second message"));
        assert!(rendered.contains("Third message"));
    }

    #[test]
    fn render_multiline_composer_input() {
        let backend = TestBackend::new(100, 30);
        let mut terminal = Terminal::new(backend).unwrap();
        let account = Account {
            id: "a".to_string(),
            username: "owner".to_string(),
            display_name: "Owner".to_string(),
            role: Role::Owner,
            activated: true,
        };
        let mut ui = UiState {
            mode: UiMode::Compose,
            ..UiState::default()
        };
        ui.composer.buffer = "hello\nworld".to_string();
        ui.composer.cursor = ui.composer.buffer.len();

        terminal
            .draw(|frame| draw(frame, &account, &Snapshot::default(), &mut ui, &[]))
            .unwrap();
        let rendered = format!("{:?}", terminal.backend().buffer());
        assert!(rendered.contains("hello"));
        assert!(rendered.contains("world"));
        assert!(rendered.contains("shift-enter newline"));
    }
}
