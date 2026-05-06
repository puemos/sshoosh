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

use crate::color::{rgb_to_xterm256, xterm256_to_rgb};

const MOUSE_ENABLE: &[u8] = b"\x1b[?1000h\x1b[?1002h\x1b[?1006h";
const MOUSE_DISABLE: &[u8] = b"\x1b[?1006l\x1b[?1003l\x1b[?1002l\x1b[?1000l";
const KEYBOARD_ENHANCEMENTS_ENABLE: &[u8] = b"\x1b[>1u";
const KEYBOARD_ENHANCEMENTS_DISABLE: &[u8] = b"\x1b[<u";
const NOTIFICATION_TITLE_LIMIT: usize = 80;
const NOTIFICATION_BODY_LIMIT: usize = 240;
const CAPABILITY_ENV_VALUE_LIMIT: usize = 128;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum ColorMode {
    #[default]
    Ansi256,
    Truecolor,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct TerminalCapabilities {
    pub color_mode: ColorMode,
    pub enhanced_keyboard: bool,
}

impl TerminalCapabilities {
    pub fn detect(term: &str, env: &TerminalCapabilityEnv) -> Self {
        let term = term.to_ascii_lowercase();
        let term_program = env.term_program.as_deref().map(str::to_ascii_lowercase);
        let apple_terminal = term_program.as_deref() == Some("apple_terminal");
        let has_truecolor = !apple_terminal
            && (env.colorterm.as_deref().is_some_and(|value| {
                matches!(value.to_ascii_lowercase().as_str(), "truecolor" | "24bit")
            }) || term.ends_with("-direct")
                || term == "xterm-kitty"
                || env.wezterm_executable.is_some()
                || env.kitty_window_id.is_some()
                || env.ghostty_resources_dir.is_some());
        let enhanced_keyboard = !apple_terminal
            && (term == "xterm-kitty"
                || env.wezterm_executable.is_some()
                || env.kitty_window_id.is_some()
                || env.ghostty_resources_dir.is_some());
        Self {
            color_mode: if has_truecolor {
                ColorMode::Truecolor
            } else {
                ColorMode::Ansi256
            },
            enhanced_keyboard,
        }
    }
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct TerminalCapabilityEnv {
    pub colorterm: Option<String>,
    pub term_program: Option<String>,
    pub wezterm_executable: Option<String>,
    pub kitty_window_id: Option<String>,
    pub ghostty_resources_dir: Option<String>,
}

impl TerminalCapabilityEnv {
    pub fn set(&mut self, name: &str, value: &str) -> bool {
        let value = sanitize_capability_env_value(value);
        match name {
            name if name.eq_ignore_ascii_case("COLORTERM") => {
                self.colorterm = Some(value);
                true
            }
            name if name.eq_ignore_ascii_case("TERM_PROGRAM") => {
                self.term_program = Some(value);
                true
            }
            name if name.eq_ignore_ascii_case("WEZTERM_EXECUTABLE") => {
                self.wezterm_executable = Some(value);
                true
            }
            name if name.eq_ignore_ascii_case("KITTY_WINDOW_ID") => {
                self.kitty_window_id = Some(value);
                true
            }
            name if name.eq_ignore_ascii_case("GHOSTTY_RESOURCES_DIR") => {
                self.ghostty_resources_dir = Some(value);
                true
            }
            _ => false,
        }
    }
}

#[derive(Clone)]
pub struct SharedBuffer {
    inner: Arc<Mutex<Vec<u8>>>,
    color_mode: Arc<Mutex<ColorMode>>,
}

impl Default for SharedBuffer {
    fn default() -> Self {
        Self::new(ColorMode::default())
    }
}

impl SharedBuffer {
    pub fn new(color_mode: ColorMode) -> Self {
        Self {
            inner: Arc::new(Mutex::new(Vec::new())),
            color_mode: Arc::new(Mutex::new(color_mode)),
        }
    }

    pub fn set_color_mode(&self, color_mode: ColorMode) -> io::Result<()> {
        *self
            .color_mode
            .lock()
            .map_err(|_| io::Error::other("terminal color mode lock poisoned"))? = color_mode;
        Ok(())
    }

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
        let color_mode = *self
            .color_mode
            .lock()
            .map_err(|_| io::Error::other("terminal color mode lock poisoned"))?;
        let mut guard = self
            .inner
            .lock()
            .map_err(|_| io::Error::other("shared terminal buffer poisoned"))?;
        match color_mode {
            ColorMode::Truecolor => guard.extend_from_slice(buf),
            ColorMode::Ansi256 => guard.extend(downgrade_truecolor_sgr(buf)),
        }
        Ok(buf.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

pub type SshooshTerminal = Terminal<CrosstermBackend<SharedBuffer>>;

pub fn terminal(
    cols: u16,
    rows: u16,
    color_mode: ColorMode,
) -> anyhow::Result<(SshooshTerminal, SharedBuffer)> {
    let shared = SharedBuffer::new(color_mode);
    let backend = CrosstermBackend::new(shared.clone());
    let terminal = Terminal::with_options(
        backend,
        TerminalOptions {
            viewport: Viewport::Fixed(Rect::new(0, 0, cols.max(1), rows.max(1))),
        },
    )?;
    Ok((terminal, shared))
}

pub fn enter_alt_screen(mouse_enabled: bool, enhanced_keyboard: bool) -> io::Result<Vec<u8>> {
    let mut buf = Vec::new();
    crossterm::execute!(
        buf,
        terminal::EnterAlternateScreen,
        cursor::Hide,
        terminal::Clear(ClearType::All)
    )?;
    buf.extend_from_slice(b"\x1b[0m");
    if mouse_enabled {
        buf.extend_from_slice(MOUSE_ENABLE);
    }
    if enhanced_keyboard {
        buf.extend_from_slice(KEYBOARD_ENHANCEMENTS_ENABLE);
    }
    buf.extend_from_slice(b"\x1b[?2004h");
    Ok(buf)
}

pub fn leave_alt_screen(mouse_enabled: bool, enhanced_keyboard: bool) -> io::Result<Vec<u8>> {
    let mut buf = Vec::new();
    buf.extend_from_slice(b"\x1b[0m\x1b[?2004l");
    if enhanced_keyboard {
        buf.extend_from_slice(KEYBOARD_ENHANCEMENTS_DISABLE);
    }
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

pub fn osc8_hyperlink_at(
    rect: Rect,
    url: &str,
    text: &str,
    style: Style,
    color_mode: ColorMode,
) -> Vec<u8> {
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
        style_sgr(style, color_mode),
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

fn sanitize_capability_env_value(value: &str) -> String {
    value
        .chars()
        .filter(|ch| !ch.is_control())
        .take(CAPABILITY_ENV_VALUE_LIMIT)
        .collect()
}

fn style_sgr(style: Style, color_mode: ColorMode) -> String {
    let mut parts = vec!["0".to_string()];
    if let Some(fg) = style.fg.and_then(rgb_color) {
        push_sgr_color(&mut parts, "38", fg, color_mode);
    }
    if let Some(bg) = style.bg.and_then(rgb_color) {
        push_sgr_color(&mut parts, "48", bg, color_mode);
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

fn push_sgr_color(
    parts: &mut Vec<String>,
    prefix: &'static str,
    rgb: (u8, u8, u8),
    color_mode: ColorMode,
) {
    match color_mode {
        ColorMode::Truecolor => parts.push(format!("{};2;{};{};{}", prefix, rgb.0, rgb.1, rgb.2)),
        ColorMode::Ansi256 => parts.push(format!("{};5;{}", prefix, rgb_to_xterm256(rgb))),
    }
}

fn rgb_color(color: Color) -> Option<(u8, u8, u8)> {
    match color {
        Color::Rgb(r, g, b) => Some((r, g, b)),
        Color::Indexed(index) => Some(xterm256_to_rgb(index)),
        _ => None,
    }
}

fn downgrade_truecolor_sgr(bytes: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(bytes.len());
    let mut idx = 0;
    while idx < bytes.len() {
        if bytes[idx] == 0x1b
            && bytes.get(idx + 1) == Some(&b'[')
            && let Some(end_offset) = bytes[idx + 2..]
                .iter()
                .position(|byte| matches!(*byte, 0x40..=0x7e))
        {
            let end = idx + 2 + end_offset;
            if bytes[end] == b'm'
                && let Ok(params) = std::str::from_utf8(&bytes[idx + 2..end])
            {
                out.extend_from_slice(b"\x1b[");
                out.extend_from_slice(downgrade_sgr_params(params).as_bytes());
                out.push(b'm');
                idx = end + 1;
                continue;
            }
            out.extend_from_slice(&bytes[idx..=end]);
            idx = end + 1;
            continue;
        }
        out.push(bytes[idx]);
        idx += 1;
    }
    out
}

fn downgrade_sgr_params(params: &str) -> String {
    let parts = params.split(';').collect::<Vec<_>>();
    let mut out = Vec::with_capacity(parts.len());
    let mut idx = 0;
    while idx < parts.len() {
        if matches!(parts[idx], "38" | "48")
            && parts.get(idx + 1) == Some(&"2")
            && let (Some(r), Some(g), Some(b)) = (
                parts.get(idx + 2).and_then(|part| part.parse::<u8>().ok()),
                parts.get(idx + 3).and_then(|part| part.parse::<u8>().ok()),
                parts.get(idx + 4).and_then(|part| part.parse::<u8>().ok()),
            )
        {
            out.push(parts[idx].to_string());
            out.push("5".to_string());
            out.push(rgb_to_xterm256((r, g, b)).to_string());
            idx += 5;
        } else {
            out.push(parts[idx].to_string());
            idx += 1;
        }
    }
    out.join(";")
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::ErrorKind;

    #[test]
    fn alt_screen_sequences_toggle_minimal_mouse_reporting() {
        let enter =
            String::from_utf8_lossy(&enter_alt_screen(true, true).expect("enter")).into_owned();
        let leave =
            String::from_utf8_lossy(&leave_alt_screen(true, true).expect("leave")).into_owned();

        assert!(enter.contains("\x1b[0m"));
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
        assert!(leave.starts_with("\x1b[0m"));
        assert!(leave.contains("\x1b]22;default\x1b\\"));
    }

    #[test]
    fn alt_screen_can_skip_mouse_reporting() {
        let enter =
            String::from_utf8_lossy(&enter_alt_screen(false, false).expect("enter")).into_owned();
        let leave =
            String::from_utf8_lossy(&leave_alt_screen(false, false).expect("leave")).into_owned();

        assert!(!enter.contains("\x1b[?1000h"));
        assert!(!enter.contains("\x1b[?1002h"));
        assert!(!enter.contains("\x1b[?1003h"));
        assert!(!enter.contains("\x1b[?1006h"));
        assert!(!enter.contains("\x1b[>1u"));
        assert!(enter.contains("\x1b[?2004h"));
        assert!(!leave.contains("\x1b[?1000l"));
        assert!(!leave.contains("\x1b[?1002l"));
        assert!(!leave.contains("\x1b[?1003l"));
        assert!(!leave.contains("\x1b[?1006l"));
        assert!(leave.contains("\x1b[?2004l"));
        assert!(!leave.contains("\x1b[<u"));
        assert!(leave.contains("\x1b]22;default\x1b\\"));
    }

    #[test]
    fn detects_terminal_capabilities_from_safe_hints() {
        assert_eq!(
            TerminalCapabilities::detect("xterm-256color", &TerminalCapabilityEnv::default()),
            TerminalCapabilities {
                color_mode: ColorMode::Ansi256,
                enhanced_keyboard: false
            }
        );
        assert_eq!(
            TerminalCapabilities::detect("xterm-direct", &TerminalCapabilityEnv::default()),
            TerminalCapabilities {
                color_mode: ColorMode::Truecolor,
                enhanced_keyboard: false
            }
        );

        let mut env = TerminalCapabilityEnv::default();
        assert!(env.set("COLORTERM", "truecolor"));
        assert_eq!(
            TerminalCapabilities::detect("xterm-256color", &env).color_mode,
            ColorMode::Truecolor
        );

        let mut env = TerminalCapabilityEnv::default();
        assert!(env.set("WEZTERM_EXECUTABLE", "/Applications/WezTerm.app"));
        assert_eq!(
            TerminalCapabilities::detect("xterm-256color", &env),
            TerminalCapabilities {
                color_mode: ColorMode::Truecolor,
                enhanced_keyboard: true
            }
        );

        let mut env = TerminalCapabilityEnv::default();
        assert!(env.set("TERM_PROGRAM", "Apple_Terminal"));
        assert!(env.set("COLORTERM", "truecolor"));
        assert_eq!(
            TerminalCapabilities::detect("xterm-direct", &env),
            TerminalCapabilities {
                color_mode: ColorMode::Ansi256,
                enhanced_keyboard: false
            }
        );
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
            ColorMode::Truecolor,
        ))
        .into_owned();

        assert!(output.starts_with("\x1b[4;3H"));
        assert!(output.contains("38;2;1;2;3"));
        assert!(output.contains(";4m"));
        assert!(output.contains("\x1b]8;;https://example.com/\x1b\\exam\x1b]8;;\x1b\\"));
    }

    #[test]
    fn osc8_hyperlink_downgrades_rgb_for_ansi256_mode() {
        let output = String::from_utf8_lossy(&osc8_hyperlink_at(
            Rect::new(0, 0, 4, 1),
            "https://example.com/",
            "example",
            Style::default().fg(Color::Rgb(1, 2, 3)),
            ColorMode::Ansi256,
        ))
        .into_owned();

        assert!(!output.contains("38;2"));
        assert!(output.contains("38;5;"));
    }

    #[test]
    fn terminal_output_respects_color_mode() {
        use ratatui::widgets::Paragraph;

        let (mut ansi_terminal, shared) = terminal(4, 2, ColorMode::Ansi256).expect("terminal");
        ansi_terminal
            .draw(|frame| {
                frame.render_widget(
                    Paragraph::new("x").style(
                        Style::default()
                            .fg(Color::Rgb(214, 214, 214))
                            .bg(Color::Rgb(34, 37, 41)),
                    ),
                    frame.area(),
                );
            })
            .expect("draw ansi256");
        let ansi = String::from_utf8_lossy(&shared.take().expect("ansi output")).into_owned();
        assert!(!ansi.contains("38;2"));
        assert!(!ansi.contains("48;2"));
    }

    #[test]
    fn shared_buffer_downgrades_truecolor_sgr() {
        let mut shared = SharedBuffer::new(ColorMode::Ansi256);
        shared
            .write_all(b"\x1b[38;2;1;2;3;48;2;34;37;41mtext")
            .expect("write ansi256");
        let output = String::from_utf8_lossy(&shared.take().expect("take output")).into_owned();

        assert!(!output.contains("38;2"));
        assert!(!output.contains("48;2"));
        assert!(output.contains("38;5;"));
        assert!(output.contains("48;5;"));

        let mut shared = SharedBuffer::new(ColorMode::Truecolor);
        shared
            .write_all(b"\x1b[38;2;1;2;3;48;2;34;37;41mtext")
            .expect("write truecolor");
        let output = String::from_utf8_lossy(&shared.take().expect("take output")).into_owned();
        assert!(output.contains("38;2"));
        assert!(output.contains("48;2"));
    }

    #[test]
    fn shared_buffer_preserves_non_sgr_csi_sequences() {
        let mut shared = SharedBuffer::new(ColorMode::Ansi256);
        shared
            .write_all(b"\x1b[?25l\x1b[38;2;1;2;3mtext")
            .expect("write csi");
        let output = String::from_utf8_lossy(&shared.take().expect("take output")).into_owned();

        assert!(output.contains("\x1b[?25l"));
        assert!(output.contains("38;5;"));
        assert!(!output.contains("38;2"));
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
