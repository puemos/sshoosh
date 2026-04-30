use ratatui::style::{Color, Modifier, Style};

// Dark palette adapted from bensimms.moe for terminal UI roles.
pub const BG: Color = Color::Rgb(34, 37, 41); // #222529
pub const COMPOSER: Color = Color::Rgb(41, 44, 47); // #292C2F
pub const KEYCAP: Color = Color::Rgb(52, 56, 59); // #34383B
pub const BADGE: Color = Color::Rgb(36, 39, 42); // #24272A
pub const PANEL: Color = BG;
pub const ELEVATED_PANEL: Color = COMPOSER;
pub const CARD: Color = BG;
pub const MESSAGE_CARD: Color = Color::Rgb(24, 27, 29); // #181B1D
pub const MESSAGE_CARD_ROOT: Color = Color::Rgb(21, 23, 25); // #151719
pub const MESSAGE_CARD_FOCUSED: Color = Color::Rgb(29, 32, 34); // #1D2022
pub const BORDER: Color = Color::Rgb(70, 73, 73); // #464949
pub const MESSAGE_SEPARATOR: Color = Color::Rgb(56, 60, 62); // #383C3E
pub const MUTED: Color = Color::Rgb(133, 146, 137); // #859289
pub const TEXT: Color = Color::Rgb(214, 214, 214); // #D6D6D6
pub const SUBTLE: Color = Color::Rgb(219, 213, 188); // #DBD5BC
pub const ACCENT: Color = Color::Rgb(120, 182, 173); // #78B6AD
pub const ACCENT_SOFT: Color = Color::Rgb(135, 201, 229); // #87C9E5
pub const MENTION: Color = Color::Rgb(182, 160, 222); // #B6A0DE
pub const MESSAGE_ROOT_GUTTER: Color = Color::Rgb(90, 162, 255); // #5AA2FF
pub const MESSAGE_GUTTER: Color = ACCENT;
pub const MESSAGE_CURRENT_USER_GUTTER: Color = ACCENT_SOFT;
pub const MESSAGE_ERROR_GUTTER: Color = Color::Rgb(230, 126, 128); // #E67E80
pub const WARN: Color = Color::Rgb(226, 174, 162); // #E2AEA2
pub const OK: Color = ACCENT;
pub const ERROR: Color = Color::Rgb(230, 126, 128); // #E67E80

pub fn base() -> Style {
    Style::default().fg(TEXT).bg(BG)
}

pub fn panel() -> Style {
    Style::default().fg(TEXT).bg(PANEL)
}

pub fn elevated_panel() -> Style {
    Style::default().fg(TEXT).bg(ELEVATED_PANEL)
}

pub fn composer() -> Style {
    Style::default().fg(TEXT).bg(COMPOSER)
}

pub fn title() -> Style {
    Style::default()
        .fg(TEXT)
        .bg(PANEL)
        .add_modifier(Modifier::BOLD)
}

pub fn muted() -> Style {
    Style::default().fg(MUTED).bg(PANEL)
}

pub fn elevated_muted() -> Style {
    Style::default().fg(MUTED).bg(ELEVATED_PANEL)
}

pub fn section_header(active: bool) -> Style {
    Style::default()
        .fg(if active { ACCENT } else { SUBTLE })
        .bg(PANEL)
        .add_modifier(Modifier::BOLD)
}

pub fn accent() -> Style {
    Style::default()
        .fg(ACCENT)
        .bg(PANEL)
        .add_modifier(Modifier::BOLD)
}

pub fn elevated_accent() -> Style {
    Style::default()
        .fg(ACCENT)
        .bg(ELEVATED_PANEL)
        .add_modifier(Modifier::BOLD)
}

pub fn unread() -> Style {
    Style::default()
        .fg(WARN)
        .bg(PANEL)
        .add_modifier(Modifier::BOLD)
}

pub fn elevated_title() -> Style {
    Style::default()
        .fg(TEXT)
        .bg(ELEVATED_PANEL)
        .add_modifier(Modifier::BOLD)
}

pub fn elevated_unread() -> Style {
    Style::default()
        .fg(WARN)
        .bg(ELEVATED_PANEL)
        .add_modifier(Modifier::BOLD)
}

pub fn selection() -> Style {
    Style::default().fg(BG).bg(ACCENT)
}

pub fn strong_selection() -> Style {
    selection().add_modifier(Modifier::BOLD)
}

pub fn message_author_on(is_current_user: bool, bg: Color) -> Style {
    Style::default()
        .fg(if is_current_user { ACCENT_SOFT } else { ACCENT })
        .bg(bg)
        .add_modifier(Modifier::BOLD)
}

pub fn message_meta_on(bg: Color) -> Style {
    Style::default().fg(MUTED).bg(bg)
}

pub fn message_body() -> Style {
    Style::default().fg(TEXT).bg(CARD)
}

pub fn message_link() -> Style {
    Style::default()
        .fg(ACCENT_SOFT)
        .bg(CARD)
        .add_modifier(Modifier::UNDERLINED)
}

pub fn message_link_target() -> Style {
    Style::default().fg(MUTED).bg(CARD)
}

pub fn message_code() -> Style {
    Style::default().fg(SUBTLE).bg(CARD)
}

pub fn message_strong(style: Style) -> Style {
    style.add_modifier(Modifier::BOLD)
}

pub fn message_emphasis(style: Style) -> Style {
    style.add_modifier(Modifier::ITALIC)
}

pub fn message_strikethrough(style: Style) -> Style {
    style.add_modifier(Modifier::CROSSED_OUT)
}

pub fn message_card_on(bg: Color) -> Style {
    Style::default().fg(TEXT).bg(bg)
}

pub fn message_gutter(color: Color, bg: Color) -> Style {
    Style::default()
        .fg(color)
        .bg(bg)
        .add_modifier(Modifier::BOLD)
}

pub fn message_separator() -> Style {
    Style::default().fg(MESSAGE_SEPARATOR).bg(PANEL)
}

