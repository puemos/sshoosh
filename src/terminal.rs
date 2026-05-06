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

const MOUSE_ENABLE: &[u8] = b"\x1b[?1000h\x1b[?1002h\x1b[?1006h";
const MOUSE_DISABLE: &[u8] = b"\x1b[?1006l\x1b[?1003l\x1b[?1002l\x1b[?1000l";
const KEYBOARD_ENHANCEMENTS_ENABLE: &[u8] = b"\x1b[>1u";
const KEYBOARD_ENHANCEMENTS_DISABLE: &[u8] = b"\x1b[<u";
const NOTIFICATION_TITLE_LIMIT: usize = 80;
const NOTIFICATION_BODY_LIMIT: usize = 240;

#[derive(Clone, Default)]
pub struct SharedBuffer {
    inner: Arc<Mutex<Vec<u8>>>,
}

impl SharedBuffer {
    pub fn take(&self) -> io::Result<Vec<u8>> {
        let mut guard = self
            .inner
            .lock()
            .map_err(|_| io::Error::other("shared terminal buffer poisoned"))?;
        let buffer: &mut Vec<u8> = &mut guard;
        Ok(std::mem::take(buffer))
    }
}

impl Write for SharedBuffer {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        let mut guard = self
            .inner
            .lock()
            .map_err(|_| io::Error::other("shared terminal buffer poisoned"))?;
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

pub fn enter_alt_screen(mouse_enabled: bool) -> io::Result<Vec<u8>> {
    let mut buf = Vec::new();
    crossterm::execute!(
        buf,
        terminal::EnterAlternateScreen,
        cursor::Hide,
        terminal::Clear(ClearType::All)
    )?;
    if mouse_enabled {
        buf.extend_from_slice(MOUSE_ENABLE);
    }
    buf.extend_from_slice(KEYBOARD_ENHANCEMENTS_ENABLE);
    buf.extend_from_slice(b"\x1b[?2004h");
    Ok(buf)
}

pub fn leave_alt_screen(mouse_enabled: bool) -> io::Result<Vec<u8>> {
    let mut buf = Vec::new();
    buf.extend_from_slice(b"\x1b[?2004l");
    buf.extend_from_slice(KEYBOARD_ENHANCEMENTS_DISABLE);
    buf.extend_from_slice(b"\x1b]111\x1b\\");
    if mouse_enabled {
        buf.extend_from_slice(MOUSE_DISABLE);
    }
    buf.extend(pointer_shape("default"));
    crossterm::execute!(buf, cursor::Show, terminal::LeaveAlternateScreen)?;
    Ok(buf)
}

pub fn osc52_copy(text: &str) -> Vec<u8> {
    format!("\x1b]52;c;{}\x07", STANDARD.encode(text)).into_bytes()
}

pub fn terminal_title(title: &str) -> Vec<u8> {
    let title = sanitize_terminal_title(title);
    if title.is_empty() {
        Vec::new()
    } else {
        format!("\x1b]0;{title}\x07").into_bytes()
    }
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

pub fn desktop_notification(title: &str, body: &str, id: &str) -> Vec<u8> {
    let title = sanitize_notification_text(title, NOTIFICATION_TITLE_LIMIT);
    let body = sanitize_notification_text(body, NOTIFICATION_BODY_LIMIT);
    if title.is_empty() && body.is_empty() {
        return b"\x07".to_vec();
    }
    let title = if title.is_empty() {
        "sshoosh".to_string()
    } else {
        title
    };
    let notification_id = sanitize_notification_id(id);
    let mut output = Vec::new();

    output.extend(
        format!(
            "\x1b]99;i={notification_id}:e=1:d=0:p=title;{}\x1b\\",
            STANDARD.encode(&title)
        )
        .into_bytes(),
    );
    output.extend(
        format!(
            "\x1b]99;i={notification_id}:e=1:d=1:p=body;{}\x1b\\",
            STANDARD.encode(&body)
        )
        .into_bytes(),
    );
    output.extend(format!("\x1b]9;{}: {}\x1b\\", title, body).into_bytes());
    output.push(0x07);
    output
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

fn sanitize_terminal_title(value: &str) -> String {
    value
        .chars()
        .filter_map(|ch| {
            if ch == '\n' || ch == '\r' || ch == '\t' {
                Some(' ')
            } else if ch.is_control() || matches!(ch, '\u{1b}' | '\u{7}') {
                None
            } else {
                Some(ch)
            }
        })
        .collect::<String>()
        .trim()
        .to_string()
}

fn sanitize_notification_text(value: &str, limit: usize) -> String {
    let mut out = value
        .chars()
        .filter_map(|ch| {
            if ch == '\n' || ch == '\r' || ch == '\t' {
                Some(' ')
            } else if ch.is_control() || matches!(ch, '\u{1b}' | '\u{7}') {
                None
            } else {
                Some(ch)
            }
        })
        .collect::<String>();
    while out.contains("  ") {
        out = out.replace("  ", " ");
    }
    out.trim().chars().take(limit).collect()
}

fn sanitize_notification_id(id: &str) -> String {
    let id = id
        .chars()
        .filter(|ch| ch.is_ascii_alphanumeric() || matches!(*ch, '-' | '_'))
        .take(64)
        .collect::<String>();
    if id.is_empty() { "sshoosh" } else { &id }.to_string()
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
    use std::io::ErrorKind;

    #[test]
    fn alt_screen_sequences_toggle_minimal_mouse_reporting() {
        let enter = String::from_utf8_lossy(&enter_alt_screen(true).expect("enter")).into_owned();
        let leave = String::from_utf8_lossy(&leave_alt_screen(true).expect("leave")).into_owned();

        assert!(enter.contains("\x1b[?1000h"));
        assert!(enter.contains("\x1b[?1002h"));
        assert!(enter.contains("\x1b[?1006h"));
        assert!(!enter.contains("\x1b[?1003h"));
        assert!(!enter.contains("\x1b[?1015h"));
        assert!(enter.contains("\x1b[>1u"));
        assert!(enter.contains("\x1b[?2004h"));
        assert!(leave.contains("\x1b[?1006l"));
        assert!(leave.contains("\x1b[?1003l"));
        assert!(leave.contains("\x1b[?1002l"));
        assert!(leave.contains("\x1b[?1000l"));
        assert!(leave.contains("\x1b[?2004l"));
        assert!(leave.contains("\x1b[<u"));
        assert!(leave.contains("\x1b]22;default\x1b\\"));
    }

    #[test]
    fn alt_screen_can_skip_mouse_reporting() {
        let enter = String::from_utf8_lossy(&enter_alt_screen(false).expect("enter")).into_owned();
        let leave = String::from_utf8_lossy(&leave_alt_screen(false).expect("leave")).into_owned();

        assert!(!enter.contains("\x1b[?1000h"));
        assert!(!enter.contains("\x1b[?1002h"));
        assert!(!enter.contains("\x1b[?1003h"));
        assert!(!enter.contains("\x1b[?1006h"));
        assert!(enter.contains("\x1b[>1u"));
        assert!(enter.contains("\x1b[?2004h"));
        assert!(!leave.contains("\x1b[?1000l"));
        assert!(!leave.contains("\x1b[?1002l"));
        assert!(!leave.contains("\x1b[?1003l"));
        assert!(!leave.contains("\x1b[?1006l"));
        assert!(leave.contains("\x1b[?2004l"));
        assert!(leave.contains("\x1b[<u"));
        assert!(leave.contains("\x1b]22;default\x1b\\"));
    }

    #[test]
    fn shared_buffer_poisoning_returns_io_errors() {
        let shared = SharedBuffer::default();
        let poisoned = shared.clone();
        let _ = std::thread::spawn(move || {
            let _guard = poisoned.inner.lock().expect("lock shared buffer");
            panic!("poison shared buffer");
        })
        .join();

        assert_eq!(shared.take().unwrap_err().kind(), ErrorKind::Other);
        let mut writer = shared;
        assert_eq!(writer.write(b"frame").unwrap_err().kind(), ErrorKind::Other);
    }

    #[test]
    fn osc52_copy_encodes_clipboard_payload() {
        assert_eq!(
            String::from_utf8_lossy(&osc52_copy("hello")),
            "\x1b]52;c;aGVsbG8=\x07"
        );
    }

    #[test]
    fn terminal_title_sanitizes_control_characters() {
        assert_eq!(
            String::from_utf8_lossy(&terminal_title(" sshoosh\n#general\u{1b}\u{7} ")),
            "\x1b]0;sshoosh #general\x07"
        );
        assert_eq!(terminal_title("\u{1b}\u{7}"), Vec::<u8>::new());
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

    #[test]
    fn desktop_notification_emits_terminal_protocols_and_bell() {
        let output =
            String::from_utf8_lossy(&desktop_notification("New DM", "Hello Alice", "notif-1"))
                .into_owned();

        assert!(output.contains("\x1b]99;i=notif-1:e=1:d=0:p=title;TmV3IERN\x1b\\"));
        assert!(output.contains("\x1b]99;i=notif-1:e=1:d=1:p=body;SGVsbG8gQWxpY2U=\x1b\\"));
        assert!(output.contains("\x1b]9;New DM: Hello Alice\x1b\\"));
        assert!(output.ends_with('\u{7}'));
    }

    #[test]
    fn desktop_notification_sanitizes_and_truncates_payload() {
        let output = String::from_utf8_lossy(&desktop_notification(
            "Title\u{1b}\nNext",
            &"x".repeat(300),
            "bad;id!",
        ))
        .into_owned();

        assert!(output.contains("i=badid"));
        assert!(!output.contains("Title\u{1b}"));
        assert!(output.contains("\x1b]9;Title Next: "));
        assert!(!output.contains(&"x".repeat(241)));
    }
}
