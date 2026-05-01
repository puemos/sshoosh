use super::*;
pub fn apply_selection(frame: &mut Frame, ui: &mut UiState) {
    let Some(range) = ui.selection.active_range() else {
        ui.selection.text.clear();
        return;
    };
    let apply_highlight = !ui.selection.copy_requested;

    let buffer = frame.buffer_mut();
    let area = *buffer.area();
    if let Some(region) = ui.selection.message_region {
        ui.selection.text = extract_selection_text(buffer, range, region.rect, apply_highlight);
        return;
    }

    if normalize_selection_range(range, area).is_none() {
        ui.selection.text.clear();
        return;
    }
    ui.selection.text = extract_selection_text(buffer, range, area, apply_highlight);
}

fn extract_selection_text(
    buffer: &mut ratatui::buffer::Buffer,
    range: SelectionRange,
    bounds: Rect,
    apply_highlight: bool,
) -> String {
    let Some((start, end)) = normalize_selection_range(range, bounds) else {
        return String::new();
    };
    let selected_style = theme::strong_selection();
    let mut lines = Vec::new();
    for y in start.y..=end.y {
        let row_start = if y == start.y { start.x } else { bounds.x };
        let row_end = if y == end.y {
            end.x
        } else {
            bounds.x.saturating_add(bounds.width).saturating_sub(1)
        };
        if row_start > row_end {
            lines.push(String::new());
            continue;
        }

        let mut line = String::new();
        for x in row_start..=row_end {
            if let Some(cell) = buffer.cell((x, y)) {
                line.push_str(cell.symbol());
            }
        }
        if apply_highlight {
            for x in row_start..=row_end {
                if let Some(cell) = buffer.cell_mut((x, y)) {
                    cell.set_style(selected_style);
                }
            }
        }
        lines.push(line.trim_end_matches(' ').to_string());
    }
    while lines.last().is_some_and(String::is_empty) {
        lines.pop();
    }
    lines.join("\n")
}

pub(crate) fn normalize_selection_range(
    range: SelectionRange,
    area: Rect,
) -> Option<(Position, Position)> {
    if area.is_empty() {
        return None;
    }

    let (mut start, mut end) = (range.start, range.end);
    if (end.y, end.x) < (start.y, start.x) {
        std::mem::swap(&mut start, &mut end);
    }

    let right = area.x.saturating_add(area.width).saturating_sub(1);
    let bottom = area.y.saturating_add(area.height).saturating_sub(1);
    if end.y < area.y || start.y > bottom {
        return None;
    }
    let start_y = start.y.clamp(area.y, bottom);
    let end_y = end.y.clamp(area.y, bottom);
    if start_y > end_y {
        return None;
    }

    Some((
        Position {
            x: start.x.clamp(area.x, right),
            y: start_y,
        },
        Position {
            x: end.x.clamp(area.x, right),
            y: end_y,
        },
    ))
}
