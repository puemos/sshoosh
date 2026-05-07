use super::*;
use crate::app::SourceFocus;
use crate::features::messages::model::{
    LabelFeedItem, LabelFeedKind, SavedMessageItem, SavedMessageKind,
};
use crate::time_format::{
    calendar_day_key, calendar_day_label, format_human_timestamp, seconds_between,
};

const GROUP_GAP_SECONDS: i64 = 5 * 60;
const ACCOUNT_BUTTON_RENAME_WIDTH: u16 = 8;
const ACCOUNT_BUTTON_DEACTIVATE_WIDTH: u16 = 12;

#[derive(Clone, Copy, Debug)]
struct AccountKeyTableLayout {
    id_width: usize,
    label_width: usize,
    fingerprint_width: usize,
    last_used_width: usize,
    state_width: usize,
    gap: usize,
}

struct AccountInputRender<'a> {
    label: &'static str,
    value: &'a str,
    cursor: usize,
    target: AccountInputTarget,
    selected: bool,
    width: usize,
}

impl AccountKeyTableLayout {
    fn actions_col(self) -> u16 {
        (self.id_width
            + self.gap
            + self.label_width
            + self.gap
            + self.fingerprint_width
            + self.gap
            + self.last_used_width
            + self.gap
            + self.state_width
            + self.gap) as u16
    }

    fn deactivate_col(self) -> u16 {
        self.actions_col()
            .saturating_add(ACCOUNT_BUTTON_RENAME_WIDTH)
            .saturating_add(self.gap as u16)
    }
}

fn should_continue_group(
    prev_author: Option<&str>,
    prev_kind: Option<MessageKind>,
    prev_created_at: Option<&str>,
    author: &str,
    kind: MessageKind,
    created_at: Option<&str>,
) -> bool {
    let Some(prev_author) = prev_author else {
        return false;
    };
    if !prev_author.eq_ignore_ascii_case(author) {
        return false;
    }
    if matches!(prev_kind, Some(MessageKind::ThreadRoot)) || matches!(kind, MessageKind::ThreadRoot)
    {
        return false;
    }
    let (Some(prev), Some(curr)) = (prev_created_at, created_at) else {
        return true;
    };
    matches!(seconds_between(prev, curr), Some(gap) if gap.abs() <= GROUP_GAP_SECONDS)
}
pub(crate) fn draw_detail(frame: &mut Frame, area: Rect, snapshot: &Snapshot, ui: &mut UiState) {
    if matches!(ui.route, Route::Dms) {
        draw_dm_detail(frame, area, snapshot, ui);
        return;
    }
    if matches!(ui.route, Route::Account) {
        draw_account_detail(frame, area, snapshot, ui);
        return;
    }
    if matches!(ui.route, Route::Search) {
        draw_search_detail(frame, area, snapshot, ui);
        return;
    }
    if matches!(ui.route, Route::Label(_)) {
        draw_label_detail(frame, area, snapshot, ui);
        return;
    }
    if matches!(ui.route, Route::Saved) {
        draw_saved_detail(frame, area, snapshot, ui);
        return;
    }
    if matches!(ui.route, Route::Notifications) {
        draw_notifications_detail(frame, area, snapshot, ui);
        return;
    }
    frame.render_widget(Block::default().style(theme::panel()), area);
    let area = pane_inner(area);
    let selected = snapshot
        .selected_thread_id
        .as_ref()
        .and_then(|id| snapshot.threads.iter().find(|thread| &thread.id == id));
    let mut items: Vec<ListItem> = Vec::new();
    let channel_slug = snapshot
        .selected_channel_id
        .as_ref()
        .and_then(|id| snapshot.channels.iter().find(|channel| &channel.id == id))
        .map(|channel| channel.slug.as_str());
    let title = selected
        .map(|thread| thread.title.as_str())
        .unwrap_or("Thread");
    let header_meta = selected.map(|thread| {
        let last_activity = thread
            .last_activity_at
            .as_deref()
            .map(format_human_timestamp)
            .unwrap_or_else(|| "no activity".to_string());
        let plural = if thread.comment_count == 1 {
            "comment"
        } else {
            "comments"
        };
        let mut meta = format!("{} {plural} · {}", thread.comment_count, last_activity,);
        if thread.edited_at.is_some() {
            meta.push_str(" · edited");
        }
        if thread.archived_at.is_some() {
            meta.push_str(" · archived");
        }
        if thread.pinned_at.is_some() {
            meta.push_str(" · pinned");
        }
        meta
    });
    draw_thread_header(frame, area, channel_slug, title, header_meta.as_deref(), ui);
    let messages_area = pane_scroll_area(area);
    let content_area = scroll_content_area(messages_area);
    let message_width = message_content_width(content_area);
    let mut message_hits = MessageHits::default();
    let mut focused_row = None;
    let mut highlighted_row = None;
    let mut content_row = 0u16;
    if let Some(thread) = selected {
        if snapshot.comments_has_more {
            append_plain_item(
                &mut items,
                &mut content_row,
                history_prompt("Older comments available. Use /older."),
            );
        }
        append_plain_item(&mut items, &mut content_row, ListItem::new(""));

        let mut last_day: Option<String> = None;
        let mut prev_author: Option<String> = None;
        let mut prev_kind: Option<MessageKind> = None;
        let mut prev_created_at: Option<String> = None;
        let mut first_message = true;

        if !thread.body.trim().is_empty() {
            last_day = calendar_day_key(&thread.created_at);
            let focus = SourceFocus::ThreadRoot;
            if ui.pending_source_focus == Some(focus) {
                focused_row = Some(content_row);
            }
            let highlighted = ui.source_highlight == Some(focus);
            if highlighted {
                highlighted_row = Some(content_row);
            }
            let card = message_card(MessageCardSpec {
                snapshot,
                kind: MessageKind::ThreadRoot,
                header_mode: HeaderMode::Full,
                author: &thread.author,
                created_at: Some(&thread.created_at),
                edited_at: thread.edited_at.as_deref(),
                saved: false,
                reactions: &thread.reactions,
                reaction_target: Some(ReactionTarget::ThreadRoot),
                body: &thread.body,
                width: message_width,
                breadcrumb: None,
                selected: highlighted,
            });
            append_message_card(&mut items, &mut message_hits, &mut content_row, card);
            prev_author = Some(thread.author.clone());
            prev_kind = Some(MessageKind::ThreadRoot);
            prev_created_at = Some(thread.created_at.clone());
            first_message = false;
        }
        for comment in snapshot.comments.iter() {
            let day = calendar_day_key(&comment.created_at);
            let day_changed = day.is_some() && last_day.is_some() && day != last_day;
            let continue_group = !day_changed
                && should_continue_group(
                    prev_author.as_deref(),
                    prev_kind,
                    prev_created_at.as_deref(),
                    &comment.author,
                    MessageKind::Comment,
                    Some(&comment.created_at),
                );
            if day_changed && let Some(label) = calendar_day_label(&comment.created_at) {
                append_plain_item(&mut items, &mut content_row, message_gap());
                append_plain_item(
                    &mut items,
                    &mut content_row,
                    date_divider(&label, message_width),
                );
                append_plain_item(&mut items, &mut content_row, message_gap());
            } else if !continue_group && !first_message {
                append_plain_item(
                    &mut items,
                    &mut content_row,
                    message_group_divider(message_width),
                );
            }
            if day.is_some() {
                last_day = day;
            }
            let header_mode = if continue_group {
                HeaderMode::Suppressed
            } else {
                HeaderMode::Full
            };
            let focus = SourceFocus::Comment(comment.obj_index);
            if ui.pending_source_focus == Some(focus) {
                focused_row = Some(content_row);
            }
            let highlighted = ui.source_highlight == Some(focus);
            if highlighted {
                highlighted_row = Some(content_row);
            }
            let card = message_card(MessageCardSpec {
                snapshot,
                kind: MessageKind::Comment,
                header_mode,
                author: &comment.author,
                created_at: Some(&comment.created_at),
                edited_at: comment.edited_at.as_deref(),
                saved: comment.saved_at.is_some(),
                reactions: &comment.reactions,
                reaction_target: Some(ReactionTarget::Comment(comment.obj_index)),
                body: &comment.body,
                width: message_width,
                breadcrumb: None,
                selected: highlighted,
            });
            let card = with_message_card_hit(
                card,
                HitTarget::EditableMessage(EditableMessageTarget::Comment(comment.obj_index)),
            );
            append_message_card(&mut items, &mut message_hits, &mut content_row, card);
            prev_author = Some(comment.author.clone());
            prev_kind = Some(MessageKind::Comment);
            prev_created_at = Some(comment.created_at.clone());
            first_message = false;
        }
    } else {
        ui.hit_map.push(messages_area, HitTarget::DetailScroll);
        ui.detail_scroll_metrics = DetailScrollMetrics::default();
        render_empty_state(
            frame,
            messages_area,
            &mut ui.detail_scroll,
            empty_thread_lines(snapshot),
        );
        return;
    }
    ui.hit_map.push(messages_area, HitTarget::DetailScroll);
    apply_pending_source_focus(
        ui,
        focused_row,
        matches!(
            ui.pending_source_focus,
            Some(SourceFocus::ThreadRoot | SourceFocus::Comment(_))
        ) && snapshot.comments_has_more,
    );
    if ui.detail_selection_scroll_pending {
        ensure_scroll_row_visible(&mut ui.detail_scroll, highlighted_row, messages_area.height);
        ui.detail_selection_scroll_pending = false;
    }
    ui.detail_scroll_metrics =
        render_scroll_items(frame, messages_area, items, &mut ui.detail_scroll);
    register_message_hits(ui, content_area, message_hits, ui.detail_scroll.offset().y);
}

pub(crate) fn draw_account_detail(
    frame: &mut Frame,
    area: Rect,
    snapshot: &Snapshot,
    ui: &mut UiState,
) {
    frame.render_widget(Block::default().style(theme::panel()), area);
    let area = pane_inner(area);
    let role = snapshot
        .current_role
        .map(|role| role.as_str().to_string())
        .unwrap_or_else(|| "-".to_string());
    draw_thread_header(frame, area, None, "Account Settings", Some(&role), ui);
    let messages_area = pane_scroll_area(area);
    let content_area = scroll_content_area(messages_area);
    let width = content_area.width as usize;
    let mut items: Vec<ListItem> = Vec::new();
    let mut row_hits: Vec<(u16, HitTarget)> = Vec::new();
    let mut precise_hits: Vec<(u16, u16, u16, HitTarget)> = Vec::new();
    let mut selected_row = None;
    let mut row = 0u16;

    append_plain_item(&mut items, &mut row, ListItem::new(""));
    append_account_input(
        &mut items,
        &mut row_hits,
        &mut selected_row,
        &mut row,
        AccountInputRender {
            label: "Username",
            value: &ui.account.username.buffer,
            cursor: ui.account.username.cursor,
            target: AccountInputTarget::Username,
            selected: ui.account.focus == AccountFocus::Username,
            width,
        },
    );
    append_plain_item(&mut items, &mut row, ListItem::new(""));
    append_account_input(
        &mut items,
        &mut row_hits,
        &mut selected_row,
        &mut row,
        AccountInputRender {
            label: "Display name",
            value: &ui.account.display_name.buffer,
            cursor: ui.account.display_name.cursor,
            target: AccountInputTarget::DisplayName,
            selected: ui.account.focus == AccountFocus::DisplayName,
            width,
        },
    );
    append_plain_item(&mut items, &mut row, ListItem::new(""));
    append_plain_item(&mut items, &mut row, ListItem::new(""));
    append_plain_item(
        &mut items,
        &mut row,
        ListItem::new(Line::from(Span::styled("Role", theme::muted()))),
    );
    append_plain_item(
        &mut items,
        &mut row,
        ListItem::new(account_value_box(&role, width)),
    );
    append_plain_item(&mut items, &mut row, ListItem::new(""));
    append_plain_item(&mut items, &mut row, ListItem::new(""));
    append_account_buttons(
        &mut items,
        &mut precise_hits,
        &mut selected_row,
        &mut row,
        [
            ("Save", AccountFocus::Save, HitTarget::AccountSave),
            ("Reset", AccountFocus::Reset, HitTarget::AccountReset),
        ],
        ui.account.focus,
    );
    append_plain_item(&mut items, &mut row, ListItem::new(""));
    append_account_buttons(
        &mut items,
        &mut precise_hits,
        &mut selected_row,
        &mut row,
        [(
            "Link new device",
            AccountFocus::LinkDevice,
            HitTarget::AccountLinkDevice,
        )],
        ui.account.focus,
    );
    append_plain_item(&mut items, &mut row, ListItem::new(""));
    append_plain_item(
        &mut items,
        &mut row,
        ListItem::new(Line::from(Span::styled(
            "SSH Keys",
            theme::section_header(false),
        ))),
    );
    let key_layout = account_key_table_layout(width);
    append_plain_item(
        &mut items,
        &mut row,
        ListItem::new(account_key_header_row(key_layout)),
    );
    if snapshot.my_ssh_keys.is_empty() {
        append_plain_item(
            &mut items,
            &mut row,
            ListItem::new(Line::from(Span::styled(
                "No SSH keys linked.",
                theme::muted(),
            ))),
        );
    } else {
        for (idx, key) in snapshot.my_ssh_keys.iter().enumerate() {
            let active = key.revoked_at.is_none();
            let selected_label = ui.account.focus == AccountFocus::KeyLabel(idx);
            let selected_deactivate = ui.account.focus == AccountFocus::KeyDeactivate(idx);
            if selected_label || selected_deactivate {
                selected_row = Some(row);
            }
            let line = account_key_row(key, selected_label, selected_deactivate, width, key_layout);
            let row_start = row;
            append_plain_item(&mut items, &mut row, ListItem::new(line));
            if active {
                precise_hits.push((
                    row_start,
                    key_layout.actions_col(),
                    ACCOUNT_BUTTON_RENAME_WIDTH,
                    HitTarget::AccountKeyLabel(idx),
                ));
                precise_hits.push((
                    row_start,
                    key_layout.deactivate_col(),
                    ACCOUNT_BUTTON_DEACTIVATE_WIDTH,
                    HitTarget::AccountKeyDeactivate(idx),
                ));
            }
        }
    }

    if ui.detail_selection_scroll_pending {
        ensure_scroll_row_visible(&mut ui.detail_scroll, selected_row, messages_area.height);
        ui.detail_selection_scroll_pending = false;
    }
    ui.detail_scroll_metrics =
        render_scroll_items(frame, messages_area, items, &mut ui.detail_scroll);
    let offset_y = ui.detail_scroll.offset().y;
    register_scroll_hits(
        ui,
        messages_area,
        HitTarget::DetailScroll,
        row_hits,
        offset_y,
    );
    register_precise_account_hits(ui, content_area, precise_hits, offset_y);
}

fn append_account_input(
    items: &mut Vec<ListItem<'static>>,
    row_hits: &mut Vec<(u16, HitTarget)>,
    selected_row: &mut Option<u16>,
    row: &mut u16,
    spec: AccountInputRender<'_>,
) {
    if spec.selected {
        *selected_row = Some(*row);
    }
    append_plain_item(
        items,
        row,
        ListItem::new(Line::from(Span::styled(spec.label, theme::muted()))),
    );
    let box_row = *row;
    append_plain_item(
        items,
        row,
        ListItem::new(account_input_box(
            spec.value,
            spec.cursor,
            spec.selected,
            spec.width,
        )),
    );
    row_hits.push((box_row, HitTarget::AccountInput(spec.target)));
}

fn account_input_box(value: &str, cursor: usize, selected: bool, width: usize) -> Line<'static> {
    let input_style = account_input_style(selected);
    let cursor_style = if selected {
        theme::strong_selection()
    } else {
        input_style
    };
    let inner_width = width.saturating_sub(2).max(1);
    let value = sanitize_terminal_visible_text(value);
    let cursor = cursor.min(value.len());
    let cursor = if value.is_char_boundary(cursor) {
        cursor
    } else {
        value.len()
    };
    let prefix_width = if selected {
        inner_width.saturating_sub(1)
    } else {
        inner_width
    };
    let prefix = truncate_text(&value[..cursor], prefix_width);
    let remaining_width = inner_width.saturating_sub(prefix.chars().count());
    let suffix_width = remaining_width.saturating_sub(usize::from(selected));
    let suffix = truncate_text(&value[cursor..], suffix_width);
    let used = prefix.chars().count() + suffix.chars().count() + usize::from(selected);
    let padding = inner_width.saturating_sub(used);
    let mut spans = vec![
        Span::styled(" ".to_string(), input_style),
        Span::styled(prefix, input_style),
    ];
    if selected {
        spans.push(Span::styled(" ".to_string(), cursor_style));
    }
    spans.extend([
        Span::styled(suffix, input_style),
        Span::styled(" ".repeat(padding + 1), input_style),
    ]);
    Line::from(spans)
}

fn account_value_box(value: &str, width: usize) -> Line<'static> {
    let style = account_input_style(false);
    let inner_width = width.saturating_sub(2).max(1);
    let text = truncate_text(sanitize_terminal_visible_text(value), inner_width);
    let padding = inner_width.saturating_sub(text.chars().count());
    Line::from(vec![
        Span::styled(" ".to_string(), style),
        Span::styled(text, style),
        Span::styled(" ".repeat(padding + 1), style),
    ])
}

fn account_input_style(selected: bool) -> Style {
    let mut style = Style::default()
        .fg(theme::text_color())
        .bg(theme::composer_bg());
    if selected {
        style = style.add_modifier(Modifier::BOLD);
    }
    style
}

fn account_button_style(selected: bool) -> Style {
    if selected {
        theme::strong_selection()
    } else {
        Style::default()
            .fg(theme::text_color())
            .bg(theme::keycap())
            .add_modifier(Modifier::BOLD)
    }
}

fn append_account_buttons<const N: usize>(
    items: &mut Vec<ListItem<'static>>,
    precise_hits: &mut Vec<(u16, u16, u16, HitTarget)>,
    selected_row: &mut Option<u16>,
    row: &mut u16,
    buttons: [(&'static str, AccountFocus, HitTarget); N],
    focus: AccountFocus,
) {
    let row_start = *row;
    let mut spans = Vec::new();
    let mut col = 0u16;
    for (idx, (label, button_focus, target)) in buttons.into_iter().enumerate() {
        if idx > 0 {
            spans.push(Span::raw("  "));
            col = col.saturating_add(2);
        }
        let selected = focus == button_focus;
        if selected {
            *selected_row = Some(row_start);
        }
        let text = account_button_text(label);
        let width = text.chars().count() as u16;
        spans.push(Span::styled(text, account_button_style(selected)));
        precise_hits.push((row_start, col, width, target));
        col = col.saturating_add(width);
    }
    append_plain_item(items, row, ListItem::new(Line::from(spans)));
}

fn account_button_text(label: &str) -> String {
    format!(" {label} ")
}

fn account_key_row(
    key: &crate::features::accounts::model::SshKeySummary,
    selected_label: bool,
    selected_deactivate: bool,
    width: usize,
    layout: AccountKeyTableLayout,
) -> Line<'static> {
    let id = key.id.chars().take(8).collect::<String>();
    let label = key.label.as_deref().unwrap_or("-");
    let fingerprint = truncate_middle(&key.fingerprint, layout.fingerprint_width.saturating_sub(1));
    let last_used = key
        .last_used_at
        .as_deref()
        .map(format_human_timestamp)
        .unwrap_or_else(|| "never".to_string());
    let state = if key.revoked_at.is_some() {
        "inactive"
    } else {
        "active"
    };
    let mut spans = Vec::new();
    push_account_key_cell(
        &mut spans,
        truncate_text(id, 8),
        layout.id_width,
        layout.gap,
        theme::message_body(),
    );
    push_account_key_cell(
        &mut spans,
        truncate_text(label, layout.label_width.saturating_sub(1)),
        layout.label_width,
        layout.gap,
        theme::message_body(),
    );
    push_account_key_cell(
        &mut spans,
        fingerprint,
        layout.fingerprint_width,
        layout.gap,
        theme::muted(),
    );
    push_account_key_cell(
        &mut spans,
        truncate_text(last_used, layout.last_used_width.saturating_sub(1)),
        layout.last_used_width,
        layout.gap,
        theme::muted(),
    );
    push_account_key_cell(
        &mut spans,
        state.to_string(),
        layout.state_width,
        layout.gap,
        if key.revoked_at.is_some() {
            theme::muted()
        } else {
            theme::accent()
        },
    );
    if key.revoked_at.is_none() {
        spans.push(Span::styled(
            account_button_text("Rename"),
            account_button_style(selected_label),
        ));
        spans.push(Span::raw(" ".repeat(layout.gap)));
        spans.push(Span::styled(
            account_button_text("Deactivate"),
            account_button_style(selected_deactivate),
        ));
    }
    let text_width = account_spans_width(&spans);
    if text_width < width {
        spans.push(Span::raw(" ".repeat(width - text_width)));
    }
    Line::from(spans)
}

fn account_key_table_layout(width: usize) -> AccountKeyTableLayout {
    if width >= 112 {
        AccountKeyTableLayout {
            id_width: 10,
            label_width: 18,
            fingerprint_width: 26,
            last_used_width: 14,
            state_width: 10,
            gap: 2,
        }
    } else {
        AccountKeyTableLayout {
            id_width: 9,
            label_width: 11,
            fingerprint_width: 16,
            last_used_width: 8,
            state_width: 7,
            gap: 1,
        }
    }
}

fn account_key_header_row(layout: AccountKeyTableLayout) -> Line<'static> {
    let mut spans = Vec::new();
    push_account_key_cell(
        &mut spans,
        "id",
        layout.id_width,
        layout.gap,
        theme::muted(),
    );
    push_account_key_cell(
        &mut spans,
        "label",
        layout.label_width,
        layout.gap,
        theme::muted(),
    );
    push_account_key_cell(
        &mut spans,
        "fingerprint",
        layout.fingerprint_width,
        layout.gap,
        theme::muted(),
    );
    push_account_key_cell(
        &mut spans,
        "last used",
        layout.last_used_width,
        layout.gap,
        theme::muted(),
    );
    push_account_key_cell(
        &mut spans,
        "state",
        layout.state_width,
        layout.gap,
        theme::muted(),
    );
    spans.push(Span::styled("actions", theme::muted()));
    Line::from(spans)
}

fn push_account_key_cell(
    spans: &mut Vec<Span<'static>>,
    value: impl Into<String>,
    width: usize,
    gap: usize,
    style: Style,
) {
    let text = truncate_text(value.into(), width);
    let padding = width.saturating_sub(text.chars().count());
    spans.push(Span::styled(
        format!("{text}{}", " ".repeat(padding)),
        style,
    ));
    spans.push(Span::raw(" ".repeat(gap)));
}

fn account_spans_width(spans: &[Span<'_>]) -> usize {
    spans.iter().map(|span| span.content.chars().count()).sum()
}

fn truncate_middle(value: &str, max_chars: usize) -> String {
    let chars = value.chars().collect::<Vec<_>>();
    if chars.len() <= max_chars {
        return value.to_string();
    }
    if max_chars <= 1 {
        return "…".to_string();
    }
    let left = max_chars / 2;
    let right = max_chars.saturating_sub(left + 1);
    let mut out = chars.iter().take(left).collect::<String>();
    out.push('…');
    out.extend(
        chars
            .iter()
            .rev()
            .take(right)
            .collect::<Vec<_>>()
            .into_iter()
            .rev(),
    );
    out
}

fn register_precise_account_hits(
    ui: &mut UiState,
    area: Rect,
    hits: Vec<(u16, u16, u16, HitTarget)>,
    offset_y: u16,
) {
    let bottom = offset_y.saturating_add(area.height);
    for (row, col, width, target) in hits {
        if row < offset_y || row >= bottom {
            continue;
        }
        let Some(x) = area.x.checked_add(col) else {
            continue;
        };
        let right = area.x.saturating_add(area.width);
        if x >= right {
            continue;
        }
        let width = width.min(right.saturating_sub(x));
        if width == 0 {
            continue;
        }
        ui.hit_map.push(
            Rect::new(x, area.y + row.saturating_sub(offset_y), width, 1),
            target,
        );
    }
}

pub(crate) fn draw_search_detail(
    frame: &mut Frame,
    area: Rect,
    snapshot: &Snapshot,
    ui: &mut UiState,
) {
    frame.render_widget(Block::default().style(theme::panel()), area);
    let area = pane_inner(area);
    let title = snapshot
        .search_query
        .as_ref()
        .map(|query| format!("Search: {query}"))
        .unwrap_or_else(|| "Search".to_string());
    draw_detail_header(frame, area, &title, ui);
    let messages_area = pane_scroll_area(area);
    let mut items = Vec::new();
    let mut row_hits = Vec::new();
    if snapshot.search_results.is_empty() {
        ui.hit_map.push(messages_area, HitTarget::DetailScroll);
        ui.detail_scroll_metrics = DetailScrollMetrics::default();
        render_empty_state(
            frame,
            messages_area,
            &mut ui.detail_scroll,
            empty_search_lines(snapshot.search_query.as_deref()),
        );
        return;
    } else {
        for (idx, result) in snapshot.search_results.iter().enumerate() {
            let selected = idx == ui.search_selected;
            let style = if selected {
                theme::title()
            } else {
                theme::message_body()
            };
            let kind = match result.kind {
                SearchKind::Thread => "thread",
                SearchKind::Comment => "comment",
                SearchKind::Dm => "dm",
            };
            let row = items.len() as u16;
            let item = ListItem::new(vec![
                Line::from(vec![
                    Span::styled(format!("{:<8}", kind), theme::muted()),
                    Span::styled(sanitize_terminal_visible_text(&result.label), style),
                ]),
                Line::from(vec![
                    Span::styled(
                        format!("{}  ", sanitize_terminal_visible_text(&result.context)),
                        theme::muted(),
                    ),
                    Span::raw(sanitize_terminal_visible_text(&result.snippet)),
                ]),
                Line::from(""),
            ]);
            let height = item.height() as u16;
            items.push(item);
            for offset in 0..height {
                row_hits.push((row.saturating_add(offset), HitTarget::SearchResult(idx)));
            }
        }
        if snapshot.search_has_more {
            items.push(history_prompt("More results available. Use /more."));
        }
    }
    ui.detail_scroll_metrics =
        render_scroll_items(frame, messages_area, items, &mut ui.detail_scroll);
    register_scroll_hits(
        ui,
        messages_area,
        HitTarget::DetailScroll,
        row_hits,
        ui.detail_scroll.offset().y,
    );
}

pub(crate) fn draw_saved_detail(
    frame: &mut Frame,
    area: Rect,
    snapshot: &Snapshot,
    ui: &mut UiState,
) {
    frame.render_widget(Block::default().style(theme::panel()), area);
    let area = pane_inner(area);
    let meta = format!(
        "{} saved {}",
        snapshot.saved_count,
        if snapshot.saved_count == 1 {
            "message"
        } else {
            "messages"
        }
    );
    draw_thread_header(frame, area, None, "Saved", Some(&meta), ui);
    let messages_area = pane_scroll_area(area);
    let content_area = scroll_content_area(messages_area);
    let message_width = message_content_width(content_area);
    if snapshot.saved_messages.is_empty() {
        ui.hit_map.push(messages_area, HitTarget::DetailScroll);
        ui.detail_scroll_metrics = DetailScrollMetrics::default();
        render_empty_state(
            frame,
            messages_area,
            &mut ui.detail_scroll,
            empty_state_lines(
                "No saved messages",
                "Use /save #n or right-click a message",
                "★ Saved",
            ),
        );
        return;
    }

    let mut items: Vec<ListItem> = Vec::new();
    let mut message_hits = MessageHits::default();
    let mut content_row = 0u16;
    let mut selected_row = None;
    let mut last_day: Option<String> = None;
    for (idx, item) in snapshot.saved_messages.iter().enumerate() {
        let selected = idx == ui.saved_selected;
        if selected {
            selected_row = Some(content_row);
        }
        if idx > 0 {
            append_feed_day_separator(
                &mut items,
                &mut content_row,
                &mut last_day,
                &item.saved_at,
                message_width,
            );
        } else {
            last_day = calendar_day_key(&item.saved_at);
        }
        let kind = match item.kind {
            SavedMessageKind::Comment => MessageKind::Comment,
            SavedMessageKind::Dm => MessageKind::Dm,
        };
        let breadcrumb = breadcrumb_spans(
            saved_breadcrumb_text(snapshot, item),
            BreadcrumbMarker::None,
        );
        let card = message_card(MessageCardSpec {
            snapshot,
            kind,
            header_mode: HeaderMode::Full,
            author: &item.author,
            created_at: Some(&item.saved_at),
            edited_at: None,
            saved: false,
            reactions: &[],
            reaction_target: None,
            body: &item.body,
            width: message_width,
            breadcrumb: Some(breadcrumb),
            selected,
        });
        let card = with_message_card_hit(card, HitTarget::SavedResult(idx));
        append_message_card(&mut items, &mut message_hits, &mut content_row, card);
    }
    if snapshot.saved_has_more {
        append_plain_item(
            &mut items,
            &mut content_row,
            history_prompt("More saved messages available. Use /more."),
        );
    }
    ui.hit_map.push(messages_area, HitTarget::DetailScroll);
    if ui.detail_selection_scroll_pending {
        ensure_scroll_row_visible(&mut ui.detail_scroll, selected_row, messages_area.height);
        ui.detail_selection_scroll_pending = false;
    }
    ui.detail_scroll_metrics =
        render_scroll_items(frame, messages_area, items, &mut ui.detail_scroll);
    register_message_hits(ui, content_area, message_hits, ui.detail_scroll.offset().y);
}

pub(crate) fn draw_label_detail(
    frame: &mut Frame,
    area: Rect,
    snapshot: &Snapshot,
    ui: &mut UiState,
) {
    frame.render_widget(Block::default().style(theme::panel()), area);
    let area = pane_inner(area);
    let tag = snapshot
        .label_query
        .as_deref()
        .or(match &ui.route {
            Route::Label(tag) => Some(tag.as_str()),
            _ => None,
        })
        .unwrap_or("label");
    let count = snapshot
        .hot_labels
        .iter()
        .find(|hot| hot.tag == tag)
        .map(|hot| hot.count)
        .unwrap_or(snapshot.label_items.len() as i64);
    let meta = format!(
        "{} labeled {}",
        count,
        if count == 1 { "message" } else { "messages" }
    );
    draw_thread_header(frame, area, None, &format!("${tag}"), Some(&meta), ui);
    let messages_area = pane_scroll_area(area);
    let content_area = scroll_content_area(messages_area);
    let message_width = message_content_width(content_area);
    if snapshot.label_items.is_empty() {
        ui.hit_map.push(messages_area, HitTarget::DetailScroll);
        ui.detail_scroll_metrics = DetailScrollMetrics::default();
        render_empty_state(
            frame,
            messages_area,
            &mut ui.detail_scroll,
            empty_state_lines(
                "No label messages",
                "Use $labels in messages",
                "/label $label",
            ),
        );
        return;
    }

    let mut items: Vec<ListItem> = Vec::new();
    let mut message_hits = MessageHits::default();
    let mut content_row = 0u16;
    let mut selected_row = None;
    for (idx, item) in snapshot.label_items.iter().enumerate() {
        let selected = idx == ui.label_selected;
        if selected {
            selected_row = Some(content_row);
        }
        if idx > 0 {
            append_plain_item(&mut items, &mut content_row, message_gap());
        }
        let kind = match item.kind {
            LabelFeedKind::Thread => MessageKind::ThreadRoot,
            LabelFeedKind::Comment => MessageKind::Comment,
            LabelFeedKind::Dm => MessageKind::Dm,
        };
        let breadcrumb = breadcrumb_spans(
            label_breadcrumb_text(snapshot, item),
            BreadcrumbMarker::None,
        );
        let card = message_card(MessageCardSpec {
            snapshot,
            kind,
            header_mode: HeaderMode::Full,
            author: &item.author,
            created_at: Some(&item.created_at),
            edited_at: None,
            saved: false,
            reactions: &[],
            reaction_target: None,
            body: &item.body,
            width: message_width,
            breadcrumb: Some(breadcrumb),
            selected,
        });
        let card = with_message_card_hit(card, HitTarget::LabelResult(idx));
        append_message_card(&mut items, &mut message_hits, &mut content_row, card);
    }
    if snapshot.label_has_more {
        append_plain_item(
            &mut items,
            &mut content_row,
            history_prompt("More labeled messages available. Use /more."),
        );
    }
    ui.hit_map.push(messages_area, HitTarget::DetailScroll);
    if ui.detail_selection_scroll_pending {
        ensure_scroll_row_visible(&mut ui.detail_scroll, selected_row, messages_area.height);
        ui.detail_selection_scroll_pending = false;
    }
    ui.detail_scroll_metrics =
        render_scroll_items(frame, messages_area, items, &mut ui.detail_scroll);
    register_message_hits(ui, content_area, message_hits, ui.detail_scroll.offset().y);
}

pub(crate) fn draw_notifications_detail(
    frame: &mut Frame,
    area: Rect,
    snapshot: &Snapshot,
    ui: &mut UiState,
) {
    frame.render_widget(Block::default().style(theme::panel()), area);
    let area = pane_inner(area);
    draw_notifications_header(frame, area, snapshot, ui);
    let messages_area = notifications_scroll_area(area);
    let content_area = scroll_content_area(messages_area);
    let message_width = message_content_width(content_area);
    let visible_indices = visible_notification_indices_for_filter(snapshot, ui.notification_filter);
    if visible_indices.is_empty() {
        ui.hit_map.push(messages_area, HitTarget::DetailScroll);
        ui.detail_scroll_metrics = DetailScrollMetrics::default();
        let (title, detail) = match ui.notification_filter {
            NotificationFilter::All => {
                ("No notifications", "Mentions and replies will show up here")
            }
            NotificationFilter::Unread => (
                "No unread notifications",
                "Unread mentions and replies will show up here",
            ),
            NotificationFilter::Read => (
                "No read notifications",
                "Read notifications will show up here",
            ),
        };
        render_empty_state(
            frame,
            messages_area,
            &mut ui.detail_scroll,
            empty_state_lines(title, detail, "/notifications"),
        );
        return;
    }

    let mut items: Vec<ListItem> = Vec::new();
    let mut message_hits = MessageHits::default();
    let mut content_row = 0u16;
    let mut selected_row = None;
    let mut last_day: Option<String> = None;
    for (visible_idx, idx) in visible_indices.into_iter().enumerate() {
        let Some(notification) = snapshot.notifications.get(idx) else {
            continue;
        };
        let selected = visible_idx == ui.notifications_selected;
        if selected {
            selected_row = Some(content_row);
        }
        if visible_idx > 0 {
            append_feed_day_separator(
                &mut items,
                &mut content_row,
                &mut last_day,
                &notification.created_at,
                message_width,
            );
        } else {
            last_day = calendar_day_key(&notification.created_at);
        }
        let unread = notification.read_at.is_none();
        let kind = if notification.conversation_id.is_some() {
            MessageKind::Dm
        } else {
            MessageKind::Comment
        };
        let author = notification
            .actor_username
            .clone()
            .unwrap_or_else(|| notification.kind.clone());
        let breadcrumb = breadcrumb_spans(
            notification_breadcrumb_text(notification),
            if unread {
                BreadcrumbMarker::Unread
            } else {
                BreadcrumbMarker::Read
            },
        );
        let card = message_card(MessageCardSpec {
            snapshot,
            kind,
            header_mode: HeaderMode::Full,
            author: &author,
            created_at: Some(&notification.created_at),
            edited_at: None,
            saved: false,
            reactions: &[],
            reaction_target: None,
            body: &notification.body,
            width: message_width,
            breadcrumb: Some(breadcrumb),
            selected,
        });
        let card = with_message_card_hit(card, HitTarget::NotificationResult(visible_idx));
        append_message_card(&mut items, &mut message_hits, &mut content_row, card);
    }

    ui.hit_map.push(messages_area, HitTarget::DetailScroll);
    if ui.detail_selection_scroll_pending {
        ensure_scroll_row_visible(&mut ui.detail_scroll, selected_row, messages_area.height);
        ui.detail_selection_scroll_pending = false;
    }
    ui.detail_scroll_metrics =
        render_scroll_items(frame, messages_area, items, &mut ui.detail_scroll);
    register_message_hits(ui, content_area, message_hits, ui.detail_scroll.offset().y);
}

fn append_feed_day_separator(
    items: &mut Vec<ListItem>,
    content_row: &mut u16,
    last_day: &mut Option<String>,
    timestamp: &str,
    message_width: usize,
) {
    let day = calendar_day_key(timestamp);
    let day_changed = day.is_some() && last_day.is_some() && day != *last_day;
    if day_changed && let Some(label) = calendar_day_label(timestamp) {
        append_plain_item(items, content_row, message_gap());
        append_plain_item(items, content_row, date_divider(&label, message_width));
        append_plain_item(items, content_row, message_gap());
    } else {
        append_plain_item(items, content_row, message_group_divider(message_width));
    }
    if day.is_some() {
        *last_day = day;
    }
}

fn visible_notification_indices_for_filter(
    snapshot: &Snapshot,
    filter: NotificationFilter,
) -> Vec<usize> {
    snapshot
        .notifications
        .iter()
        .enumerate()
        .filter_map(|(idx, notification)| {
            let visible = match filter {
                NotificationFilter::All => true,
                NotificationFilter::Unread => notification.read_at.is_none(),
                NotificationFilter::Read => notification.read_at.is_some(),
            };
            visible.then_some(idx)
        })
        .collect()
}

fn notifications_scroll_area(area: Rect) -> Rect {
    let header_height = area.height.min(3);
    Rect::new(
        area.x,
        area.y.saturating_add(header_height),
        area.width,
        area.height.saturating_sub(header_height),
    )
}

fn draw_notifications_header(frame: &mut Frame, area: Rect, snapshot: &Snapshot, ui: &mut UiState) {
    if area.height == 0 || area.width == 0 {
        return;
    }
    let header = Rect::new(area.x, area.y, area.width, 1);
    let toolbar = Rect::new(
        area.x,
        area.y.saturating_add(1),
        area.width,
        area.height.saturating_sub(1).min(1),
    );
    let active = ui.active_pane == ActivePane::Detail;
    let title_style = if active {
        theme::title()
    } else {
        theme::muted()
    };
    let total = snapshot.notifications.len();
    let unread = snapshot
        .notifications
        .iter()
        .filter(|notification| notification.read_at.is_none())
        .count();
    let meta = format!("{unread} unread / {total} total");
    let spans = vec![
        Span::styled("Notifications", title_style),
        Span::styled("   ", theme::muted()),
        Span::styled(meta, theme::muted()),
    ];
    frame.render_widget(
        Paragraph::new(Line::from(spans)).style(theme::panel()),
        header,
    );
    if toolbar.is_empty() {
        return;
    }

    let mut toolbar_spans = Vec::new();
    let mut cursor = 0u16;
    for filter in [
        NotificationFilter::All,
        NotificationFilter::Unread,
        NotificationFilter::Read,
    ] {
        let active_filter = ui.notification_filter == filter;
        let label = filter.label();
        let text = format!(" {label} ");
        let width = text.chars().count() as u16;
        if cursor.saturating_add(width) > toolbar.width {
            break;
        }
        let style = if active_filter {
            theme::selection()
        } else {
            theme::muted()
        };
        toolbar_spans.push(Span::styled(text, style));
        ui.hit_map.push(
            Rect::new(toolbar.x.saturating_add(cursor), toolbar.y, width, 1),
            HitTarget::NotificationFilter(filter),
        );
        cursor = cursor.saturating_add(width);
        if cursor.saturating_add(1) < toolbar.width {
            toolbar_spans.push(Span::styled(" ", theme::muted()));
            cursor = cursor.saturating_add(1);
        }
    }
    let divider = "  │  ";
    let divider_width = divider.chars().count() as u16;
    let read_all_width = " Read all ".chars().count() as u16;
    if cursor
        .saturating_add(divider_width)
        .saturating_add(read_all_width)
        <= toolbar.width
    {
        toolbar_spans.push(Span::styled(divider, theme::muted()));
        cursor = cursor.saturating_add(divider_width);
    }
    push_notification_toolbar_action(
        &mut toolbar_spans,
        &mut cursor,
        toolbar,
        ui,
        "Read all",
        HitTarget::NotificationReadAll,
        0,
    );
    push_notification_toolbar_action(
        &mut toolbar_spans,
        &mut cursor,
        toolbar,
        ui,
        "Archive all",
        HitTarget::NotificationArchiveAll,
        3,
    );
    frame.render_widget(
        Paragraph::new(Line::from(toolbar_spans)).style(theme::panel()),
        toolbar,
    );
}

fn push_notification_toolbar_action(
    spans: &mut Vec<Span<'static>>,
    cursor: &mut u16,
    toolbar: Rect,
    ui: &mut UiState,
    label: &'static str,
    target: HitTarget,
    gap: u16,
) {
    let text = format!(" {label} ");
    let width = text.chars().count() as u16;
    let gap = if *cursor == 0 { 0 } else { gap };
    if cursor.saturating_add(gap).saturating_add(width) > toolbar.width {
        return;
    }
    if gap > 0 {
        spans.push(Span::styled(" ".repeat(gap as usize), theme::muted()));
        *cursor = cursor.saturating_add(gap);
    }
    spans.push(Span::styled(text, theme::muted()));
    ui.hit_map.push(
        Rect::new(toolbar.x.saturating_add(*cursor), toolbar.y, width, 1),
        target,
    );
    *cursor = cursor.saturating_add(width);
}

#[derive(Clone, Copy, Debug, Default)]
enum BreadcrumbMarker {
    #[default]
    None,
    Unread,
    Read,
}

fn breadcrumb_spans(text: String, marker: BreadcrumbMarker) -> Vec<Span<'static>> {
    let mut spans = Vec::new();
    match marker {
        BreadcrumbMarker::Unread => spans.push(Span::styled("● ", theme::unread())),
        BreadcrumbMarker::Read => spans.push(Span::styled("  ", theme::muted())),
        BreadcrumbMarker::None => {}
    }
    spans.push(Span::styled(
        sanitize_terminal_visible_text(&text),
        theme::muted(),
    ));
    spans
}

fn saved_breadcrumb_text(snapshot: &Snapshot, item: &SavedMessageItem) -> String {
    match item.kind {
        SavedMessageKind::Dm => {
            let actor = snapshot
                .current_username
                .as_deref()
                .unwrap_or(item.author.as_str());
            let peer = item
                .dm_peer_username
                .as_deref()
                .unwrap_or(item.source_label.strip_prefix("DM @").unwrap_or("DM"));
            format!("DM @{actor} → @{peer}")
        }
        SavedMessageKind::Comment => {
            match (item.channel_slug.as_deref(), item.thread_title.as_deref()) {
                (Some(slug), Some(title)) => format!("#{slug} › {title}"),
                _ => item.source_label.replace(" · ", " › "),
            }
        }
    }
}

fn label_breadcrumb_text(snapshot: &Snapshot, item: &LabelFeedItem) -> String {
    match item.kind {
        LabelFeedKind::Thread | LabelFeedKind::Comment => {
            match (item.channel_slug.as_deref(), item.thread_title.as_deref()) {
                (Some(slug), Some(title)) => format!("#{slug} › {title}"),
                _ => item.source_label.replace(" · ", " › "),
            }
        }
        LabelFeedKind::Dm => {
            let actor = snapshot
                .current_username
                .as_deref()
                .unwrap_or(item.author.as_str());
            let peer = item
                .dm_peer_username
                .as_deref()
                .unwrap_or(item.source_label.strip_prefix("DM @").unwrap_or("DM"));
            format!("DM @{actor} → @{peer}")
        }
    }
}

fn notification_breadcrumb_text(notification: &NotificationSummary) -> String {
    if notification.conversation_id.is_some() {
        return notification
            .actor_username
            .as_ref()
            .map(|username| format!("DM @{username}"))
            .unwrap_or_else(|| "DM".to_string());
    }
    match (
        notification.channel_slug.as_deref(),
        notification.thread_title.as_deref(),
    ) {
        (Some(slug), Some(title)) => format!("#{slug} › {title}"),
        (Some(slug), None) => format!("#{slug}"),
        _ => notification.title.clone(),
    }
}

pub(crate) fn draw_workspace_header(frame: &mut Frame, area: Rect, title: &str, ui: &UiState) {
    draw_pane_header(
        frame,
        area,
        title,
        theme::section_header(matches!(&ui.route, Route::Channel(_))),
    );
}

pub(crate) fn draw_thread_header(
    frame: &mut Frame,
    area: Rect,
    channel_slug: Option<&str>,
    title: &str,
    meta: Option<&str>,
    ui: &UiState,
) {
    if area.height == 0 || area.width == 0 {
        return;
    }
    let header = Rect::new(area.x, area.y, area.width, 1);
    let active = ui.active_pane == ActivePane::Detail;
    let title_style = if active {
        theme::title()
    } else {
        theme::muted()
    };
    let mut spans = Vec::new();
    if let Some(slug) = channel_slug {
        spans.push(Span::styled(
            format!("#{}", sanitize_terminal_visible_text(slug)),
            title_style,
        ));
        spans.push(Span::styled(" › ", theme::muted()));
    }
    spans.push(Span::styled(
        sanitize_terminal_visible_text(title),
        title_style,
    ));
    if let Some(meta) = meta.filter(|value| !value.is_empty()) {
        let used: usize = spans.iter().map(|span| span.content.chars().count()).sum();
        let remaining = (area.width as usize).saturating_sub(used);
        if remaining > 6 {
            spans.push(Span::styled("   ", theme::muted()));
            let meta_max = remaining.saturating_sub(3);
            let truncated = truncate_text(meta, meta_max);
            spans.push(Span::styled(truncated, theme::muted()));
        }
    }
    frame.render_widget(
        Paragraph::new(Line::from(spans)).style(theme::panel()),
        header,
    );
}

pub(crate) fn draw_detail_header(frame: &mut Frame, area: Rect, title: &str, ui: &UiState) {
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

pub(crate) fn draw_pane_header(frame: &mut Frame, area: Rect, title: &str, style: Style) {
    if area.height == 0 || area.width == 0 {
        return;
    }
    let header = Rect::new(area.x, area.y, area.width, 1);
    frame.render_widget(
        Paragraph::new(Line::from(Span::styled(
            sanitize_terminal_visible_text(title),
            style,
        )))
        .style(theme::panel()),
        header,
    );
}

pub(crate) fn pane_scroll_area(area: Rect) -> Rect {
    let header_height = area.height.min(2);
    Rect::new(
        area.x,
        area.y.saturating_add(header_height),
        area.width,
        area.height.saturating_sub(header_height),
    )
}

pub(crate) fn scroll_content_area(area: Rect) -> Rect {
    Rect::new(area.x, area.y, area.width.saturating_sub(2), area.height)
}

pub(crate) fn ensure_scroll_row_visible(
    state: &mut ScrollViewState,
    row: Option<u16>,
    viewport_height: u16,
) {
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

pub(crate) fn register_scroll_hits(
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

pub(crate) fn render_scroll_items(
    frame: &mut Frame,
    area: Rect,
    items: Vec<ListItem>,
    state: &mut ScrollViewState,
) -> DetailScrollMetrics {
    if area.is_empty() {
        return DetailScrollMetrics::default();
    }
    frame.render_widget(Block::default().style(theme::panel()), area);
    let total_height = items.iter().map(ListItem::height).sum::<usize>().max(1);
    let content_height = total_height.min(u16::MAX as usize) as u16;
    let max_y_offset = content_height.saturating_sub(area.height);
    let offset_y = state.offset().y.min(max_y_offset);
    state.set_offset(Position { x: 0, y: offset_y });

    let show_vertical_scrollbar = content_height > area.height;
    let content_area = if show_vertical_scrollbar {
        Rect::new(area.x, area.y, area.width.saturating_sub(1), area.height)
    } else {
        area
    };
    let viewport_bottom = offset_y as usize + area.height as usize;
    let mut cursor = 0usize;
    let mut top_trim = 0u16;
    let mut visible_height = 0usize;
    let mut visible_items = Vec::new();
    for item in items {
        let item_height = item.height();
        let item_bottom = cursor.saturating_add(item_height);
        if item_bottom <= offset_y as usize {
            cursor = item_bottom;
            continue;
        }
        if cursor >= viewport_bottom {
            break;
        }
        if visible_items.is_empty() {
            top_trim = (offset_y as usize)
                .saturating_sub(cursor)
                .min(u16::MAX as usize) as u16;
        }
        visible_height = visible_height.saturating_add(item_height);
        visible_items.push(item);
        cursor = item_bottom;
    }

    if !visible_items.is_empty() && !content_area.is_empty() {
        let visible_content_height = visible_height
            .max(top_trim as usize + area.height as usize)
            .min(u16::MAX as usize) as u16;
        let mut visible_state = ScrollViewState::with_offset(Position { x: 0, y: top_trim });
        let mut scroll_view =
            ScrollView::new(Size::new(content_area.width, visible_content_height))
                .vertical_scrollbar_visibility(ScrollbarVisibility::Never)
                .horizontal_scrollbar_visibility(ScrollbarVisibility::Never);
        scroll_view.render_widget(
            List::new(visible_items)
                .style(theme::panel())
                .highlight_style(theme::panel()),
            Rect::new(0, 0, content_area.width, visible_content_height),
        );
        frame.render_stateful_widget(scroll_view, content_area, &mut visible_state);
    }

    if show_vertical_scrollbar {
        let mut scrollbar_state =
            ScrollbarState::new(content_height.saturating_sub(area.height) as usize)
                .position(offset_y as usize);
        frame.render_stateful_widget(
            Scrollbar::new(ScrollbarOrientation::VerticalRight),
            area,
            &mut scrollbar_state,
        );
    }

    DetailScrollMetrics {
        offset_y,
        max_y_offset,
        viewport_height: area.height,
    }
}

pub(crate) fn draw_dm_detail(frame: &mut Frame, area: Rect, snapshot: &Snapshot, ui: &mut UiState) {
    frame.render_widget(Block::default().style(theme::panel()), area);
    let area = pane_inner(area);
    let selected = snapshot
        .selected_conversation_id
        .as_ref()
        .and_then(|id| snapshot.conversations.iter().find(|dm| &dm.id == id));
    let title = snapshot
        .selected_conversation_id
        .as_ref()
        .and_then(|id| snapshot.conversations.iter().find(|dm| &dm.id == id))
        .map(|dm| format!("DM @{}", dm.peer_username))
        .unwrap_or_else(|| "DMs".to_string());
    let meta = selected.map(|dm| {
        let last_activity = dm
            .last_activity_at
            .as_deref()
            .map(format_human_timestamp)
            .unwrap_or_else(|| "no activity".to_string());
        if dm.unread_count > 0 {
            format!("{} unread · {}", dm.unread_count, last_activity)
        } else {
            last_activity
        }
    });
    draw_thread_header(frame, area, None, &title, meta.as_deref(), ui);
    let messages_area = pane_scroll_area(area);
    let content_area = scroll_content_area(messages_area);
    let message_width = message_content_width(content_area);
    let mut items: Vec<ListItem> = Vec::new();
    let mut message_hits = MessageHits::default();
    let mut focused_row = None;
    let mut highlighted_row = None;
    let mut content_row = 0u16;
    if snapshot.conversation_messages_has_more {
        append_plain_item(
            &mut items,
            &mut content_row,
            history_prompt("Older messages available. Use /older."),
        );
    }
    if snapshot.conversation_messages.is_empty() {
        ui.hit_map.push(messages_area, HitTarget::DetailScroll);
        ui.detail_scroll_metrics = DetailScrollMetrics::default();
        render_empty_state(
            frame,
            messages_area,
            &mut ui.detail_scroll,
            empty_dm_lines(selected.is_some()),
        );
        return;
    } else {
        let mut prev_author: Option<String> = None;
        let mut prev_kind: Option<MessageKind> = None;
        let mut prev_created_at: Option<String> = None;
        for (idx, message) in snapshot.conversation_messages.iter().enumerate() {
            let continue_group = should_continue_group(
                prev_author.as_deref(),
                prev_kind,
                prev_created_at.as_deref(),
                &message.author,
                MessageKind::Dm,
                Some(&message.created_at),
            );
            if idx > 0 && !continue_group {
                append_plain_item(
                    &mut items,
                    &mut content_row,
                    message_group_divider(message_width),
                );
            }
            let header_mode = if continue_group {
                HeaderMode::Suppressed
            } else {
                HeaderMode::Full
            };
            let focus = SourceFocus::Dm(message.obj_index);
            if ui.pending_source_focus == Some(focus) {
                focused_row = Some(content_row);
            }
            let highlighted = ui.source_highlight == Some(focus);
            if highlighted {
                highlighted_row = Some(content_row);
            }
            let card = message_card(MessageCardSpec {
                snapshot,
                kind: MessageKind::Dm,
                header_mode,
                author: &message.author,
                created_at: Some(&message.created_at),
                edited_at: message.edited_at.as_deref(),
                saved: message.saved_at.is_some(),
                reactions: &message.reactions,
                reaction_target: Some(ReactionTarget::Dm(message.obj_index)),
                body: &message.body,
                width: message_width,
                breadcrumb: None,
                selected: highlighted,
            });
            let card = with_message_card_hit(
                card,
                HitTarget::EditableMessage(EditableMessageTarget::Dm(message.obj_index)),
            );
            append_message_card(&mut items, &mut message_hits, &mut content_row, card);
            prev_author = Some(message.author.clone());
            prev_kind = Some(MessageKind::Dm);
            prev_created_at = Some(message.created_at.clone());
        }
    }
    ui.hit_map.push(messages_area, HitTarget::DetailScroll);
    apply_pending_source_focus(
        ui,
        focused_row,
        matches!(ui.pending_source_focus, Some(SourceFocus::Dm(_)))
            && snapshot.conversation_messages_has_more,
    );
    if ui.detail_selection_scroll_pending {
        ensure_scroll_row_visible(&mut ui.detail_scroll, highlighted_row, messages_area.height);
        ui.detail_selection_scroll_pending = false;
    }
    ui.detail_scroll_metrics =
        render_scroll_items(frame, messages_area, items, &mut ui.detail_scroll);
    register_message_hits(ui, content_area, message_hits, ui.detail_scroll.offset().y);
}

fn apply_pending_source_focus(ui: &mut UiState, focused_row: Option<u16>, capped: bool) {
    if let Some(row) = focused_row {
        ui.detail_scroll.set_offset(Position { x: 0, y: row });
        ui.pending_source_focus = None;
    } else if capped {
        ui.pending_source_focus = None;
        ui.source_highlight = None;
        ui.banner = Some(Banner::err("Message is older than loaded history"));
    }
}

pub(crate) fn history_prompt(text: &'static str) -> ListItem<'static> {
    ListItem::new(Line::from(Span::styled(text, theme::muted())))
}

fn render_empty_state(
    frame: &mut Frame,
    area: Rect,
    state: &mut ScrollViewState,
    lines: Vec<Line<'static>>,
) {
    if area.is_empty() {
        return;
    }
    state.set_offset(Position { x: 0, y: 0 });
    let height = (lines.len() as u16).min(area.height);
    let y = area
        .y
        .saturating_add(area.height.saturating_sub(height) / 3);
    frame.render_widget(
        Paragraph::new(lines)
            .style(theme::panel())
            .alignment(Alignment::Center)
            .wrap(Wrap { trim: true }),
        Rect::new(area.x, y, area.width, height),
    );
}

fn empty_thread_lines(snapshot: &Snapshot) -> Vec<Line<'static>> {
    if snapshot.channels.is_empty() {
        return empty_state_lines(
            "No channels yet",
            "Create a place for the first thread",
            "/channel new name",
        );
    }
    if snapshot.threads.is_empty() {
        return empty_state_lines(
            "No threads in this channel",
            "Start the conversation here",
            "/thread new title",
        );
    }
    empty_state_lines(
        "Select a thread",
        "Browse threads on the left",
        "/thread new title",
    )
}

fn empty_search_lines(query: Option<&str>) -> Vec<Line<'static>> {
    if query.is_some_and(|value| !value.trim().is_empty()) {
        return empty_state_lines("No results", "Try different terms", "/search query");
    }
    empty_state_lines(
        "Search messages",
        "Find threads, comments, and DMs",
        "/search query",
    )
}

fn empty_dm_lines(has_selected_dm: bool) -> Vec<Line<'static>> {
    if has_selected_dm {
        return empty_state_lines(
            "No messages yet",
            "Type below to start the DM",
            "/dm open @user",
        );
    }
    empty_state_lines(
        "Select a DM",
        "Open an existing conversation",
        "/dm open @user",
    )
}

fn empty_state_lines(
    title: &'static str,
    detail: &'static str,
    command: &'static str,
) -> Vec<Line<'static>> {
    vec![
        Line::from(Span::styled(title, theme::title())),
        Line::from(""),
        Line::from(Span::styled(detail, theme::muted())),
        Line::from(vec![
            Span::styled("Use ", theme::muted()),
            Span::styled(command, theme::accent()),
        ]),
    ]
}
