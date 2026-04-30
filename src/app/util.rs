fn mouse_position(mouse: MouseEvent) -> Position {
    Position {
        x: mouse.column,
        y: mouse.row,
    }
}

fn clamp_index(current: usize, delta: isize, len: usize) -> usize {
    if len == 0 {
        return 0;
    }
    let next = current as isize + delta;
    next.clamp(0, len.saturating_sub(1) as isize) as usize
}

fn cursor_for_display_position(
    buffer: &str,
    width: usize,
    target_line: usize,
    target_col: usize,
) -> usize {
    let width = width.max(1);
    let mut line = 0;
    let mut col = 0;

    for (idx, ch) in buffer.char_indices() {
        if ch == '\n' {
            if line == target_line {
                return idx;
            }
            line += 1;
            col = 0;
            continue;
        }

        if col >= width {
            if line == target_line {
                return idx;
            }
            line += 1;
            col = 0;
        }

        if line == target_line && col >= target_col {
            return idx;
        }
        col += 1;
    }

    buffer.len()
}
