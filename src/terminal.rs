use std::{
    io::{self, Write},
    sync::{Arc, Mutex},
};

use base64::{Engine as _, engine::general_purpose::STANDARD};
use crossterm::{
    cursor,
    terminal::{self, ClearType},
};
use ratatui::{
    Terminal, TerminalOptions, Viewport,
    backend::CrosstermBackend,
    layout::Rect,
    style::{Color, Modifier, Style},
};

const MOUSE_ENABLE: &[u8] = b"\x1b[?1000h\x1b[?1002h\x1b[?1003h\x1b[?1006h";
const MOUSE_DISABLE: &[u8] = b"\x1b[?1006l\x1b[?1003l\x1b[?1002l\x1b[?1000l";

#[derive(Clone, Default)]
pub struct SharedBuffer {
    inner: Arc<Mutex<Vec<u8>>>,
}

impl SharedBuffer {
    pub fn take(&self) -> Vec<u8> {
        let mut guard = self.inner.lock().expect("shared terminal buffer poisoned");
        std::mem::take(&mut *guard)
    }
}

impl Write for SharedBuffer {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        let mut guard = self.inner.lock().expect("shared terminal buffer poisoned");
        guard.extend_from_slice(buf);
        Ok(buf.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

pub type SshooshTerminal = Terminal<CrosstermBackend<SharedBuffer>>;

pub fn terminal(cols: u16, rows: u16) -> anyhow::Result<(SshooshTerminal, SharedBuffer)> {
    let shared = SharedBuffer::default();
    let backend = CrosstermBackend::new(shared.clone());
    let terminal = Terminal::with_options(
        backend,
        TerminalOptions {
            viewport: Viewport::Fixed(Rect::new(0, 0, cols.max(1), rows.max(1))),
        },
    )?;
    Ok((terminal, shared))
}

pub fn enter_alt_screen(mouse_enabled: bool) -> Vec<u8> {
    let mut buf = Vec::new();
    crossterm::execute!(
        buf,
        terminal::EnterAlternateScreen,
        cursor::Hide,
        terminal::Clear(ClearType::All)
    )
    .expect("write terminal enter sequence");
    if mouse_enabled {
        buf.extend_from_slice(MOUSE_ENABLE);
    }
    buf.extend_from_slice(b"\x1b[?2004h");
    buf
}

pub fn leave_alt_screen(mouse_enabled: bool) -> Vec<u8> {
    let mut buf = Vec::new();
    buf.extend_from_slice(b"\x1b[?2004l\x1b]111\x1b\\");
    if mouse_enabled {
        buf.extend_from_slice(MOUSE_DISABLE);
    }
    buf.extend(pointer_shape("default"));
    crossterm::execute!(buf, cursor::Show, terminal::LeaveAlternateScreen)
        .expect("write terminal leave sequence");
    buf
}

pub fn osc52_copy(text: &str) -> Vec<u8> {
    format!("\x1b]52;c;{}\x07", STANDARD.encode(text)).into_bytes()
}

pub fn pointer_shape(shape: &str) -> Vec<u8> {
    let shape = shape
        .chars()
        .filter(|ch| ch.is_ascii_lowercase() || *ch == '-')
        .collect::<String>();
    if shape.is_empty() {
        Vec::new()
    } else {
        format!("\x1b]22;{shape}\x1b\\").into_bytes()
    }
}

pub fn osc8_hyperlink_at(rect: Rect, url: &str, text: &str, style: Style) -> Vec<u8> {
    let url = sanitize_osc8(url);
    let text = sanitize_visible_text(text);
    if url.is_empty() || text.is_empty() || rect.is_empty() {
        return Vec::new();
    }
    let text = text.chars().take(rect.width as usize).collect::<String>();
    format!(
        "\x1b[{};{}H{}\x1b]8;;{}\x1b\\{}\x1b]8;;\x1b\\\x1b[0m",
        rect.y.saturating_add(1),
        rect.x.saturating_add(1),
        style_sgr(style),
        url,
        text
    )
    .into_bytes()
}

fn sanitize_osc8(value: &str) -> String {
    value
        .chars()
        .filter(|ch| !matches!(*ch, '\u{1b}' | '\u{7}' | '\\') && !ch.is_control())
        .collect()
}

fn sanitize_visible_text(value: &str) -> String {
    value
        .chars()
        .filter(|ch| !ch.is_control())
        .collect::<String>()
}

fn style_sgr(style: Style) -> String {
    let mut parts = vec!["0".to_string()];
    if let Some(fg) = style.fg.and_then(rgb_color) {
        parts.push(format!("38;2;{};{};{}", fg.0, fg.1, fg.2));
    }
    if let Some(bg) = style.bg.and_then(rgb_color) {
        parts.push(format!("48;2;{};{};{}", bg.0, bg.1, bg.2));
    }
    if style.add_modifier.contains(Modifier::BOLD) {
        parts.push("1".to_string());
    }
    if style.add_modifier.contains(Modifier::ITALIC) {
        parts.push("3".to_string());
    }
    if style.add_modifier.contains(Modifier::UNDERLINED) {
        parts.push("4".to_string());
    }
    if style.add_modifier.contains(Modifier::CROSSED_OUT) {
        parts.push("9".to_string());
    }
    format!("\x1b[{}m", parts.join(";"))
}

fn rgb_color(color: Color) -> Option<(u8, u8, u8)> {
    match color {
        Color::Rgb(r, g, b) => Some((r, g, b)),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn alt_screen_sequences_toggle_minimal_mouse_reporting() {
        let enter = String::from_utf8_lossy(&enter_alt_screen(true)).into_owned();
        let leave = String::from_utf8_lossy(&leave_alt_screen(true)).into_owned();

        assert!(enter.contains("\x1b[?1000h"));
        assert!(enter.contains("\x1b[?1002h"));
        assert!(enter.contains("\x1b[?1003h"));
        assert!(enter.contains("\x1b[?1006h"));
        assert!(!enter.contains("\x1b[?1015h"));
        assert!(enter.contains("\x1b[?2004h"));
        assert!(leave.contains("\x1b[?1006l"));
        assert!(leave.contains("\x1b[?1003l"));
        assert!(leave.contains("\x1b[?1002l"));
        assert!(leave.contains("\x1b[?1000l"));
        assert!(leave.contains("\x1b[?2004l"));
        assert!(leave.contains("\x1b]22;default\x1b\\"));
    }

    #[test]
    fn alt_screen_can_skip_mouse_reporting() {
        let enter = String::from_utf8_lossy(&enter_alt_screen(false)).into_owned();
        let leave = String::from_utf8_lossy(&leave_alt_screen(false)).into_owned();

        assert!(!enter.contains("\x1b[?1000h"));
        assert!(!enter.contains("\x1b[?1002h"));
        assert!(!enter.contains("\x1b[?1003h"));
        assert!(!enter.contains("\x1b[?1006h"));
        assert!(enter.contains("\x1b[?2004h"));
        assert!(!leave.contains("\x1b[?1000l"));
        assert!(!leave.contains("\x1b[?1002l"));
        assert!(!leave.contains("\x1b[?1003l"));
        assert!(!leave.contains("\x1b[?1006l"));
        assert!(leave.contains("\x1b[?2004l"));
        assert!(leave.contains("\x1b]22;default\x1b\\"));
    }

    #[test]
    fn osc52_copy_encodes_clipboard_payload() {
        assert_eq!(
            String::from_utf8_lossy(&osc52_copy("hello")),
            "\x1b]52;c;aGVsbG8=\x07"
        );
    }

    #[test]
    fn pointer_shape_uses_osc22() {
        assert_eq!(
            String::from_utf8_lossy(&pointer_shape("pointer")),
            "\x1b]22;pointer\x1b\\"
        );
        assert_eq!(
            String::from_utf8_lossy(&pointer_shape("bad\u{7}shape")),
            "\x1b]22;badshape\x1b\\"
        );
    }

    #[test]
    fn osc8_hyperlink_positions_and_wraps_visible_text() {
        let output = String::from_utf8_lossy(&osc8_hyperlink_at(
            Rect::new(2, 3, 4, 1),
            "https://example.com/\u{7}",
            "example",
            Style::default()
                .fg(Color::Rgb(1, 2, 3))
                .add_modifier(Modifier::UNDERLINED),
        ))
        .into_owned();

        assert!(output.starts_with("\x1b[4;3H"));
        assert!(output.contains("38;2;1;2;3"));
        assert!(output.contains(";4m"));
        assert!(output.contains("\x1b]8;;https://example.com/\x1b\\exam\x1b]8;;\x1b\\"));
    }
}
