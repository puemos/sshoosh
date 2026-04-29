use ratatui::style::{Color, Modifier, Style};

// Dark palette adapted from bensimms.moe for terminal UI roles.
pub const BG: Color = Color::Rgb(34, 37, 41); // #222529
pub const TOPBAR: Color = BG;
pub const TOPBAR_DARK: Color = BG;
pub const COMPOSER: Color = Color::Rgb(41, 44, 47); // #292C2F
pub const PANEL: Color = BG;
pub const CARD: Color = BG;
pub const BORDER: Color = Color::Rgb(70, 73, 73); // #464949
pub const MUTED: Color = Color::Rgb(133, 146, 137); // #859289
pub const TEXT: Color = Color::Rgb(214, 214, 214); // #D6D6D6
pub const SUBTLE: Color = Color::Rgb(219, 213, 188); // #DBD5BC
pub const ACCENT: Color = Color::Rgb(120, 182, 173); // #78B6AD
pub const ACCENT_SOFT: Color = Color::Rgb(135, 201, 229); // #87C9E5
pub const WARN: Color = Color::Rgb(226, 174, 162); // #E2AEA2
pub const OK: Color = ACCENT;
pub const ERROR: Color = Color::Rgb(230, 126, 128); // #E67E80

pub fn base() -> Style {
    Style::default().fg(TEXT).bg(BG)
}

pub fn panel() -> Style {
    Style::default().fg(TEXT).bg(PANEL)
}

pub fn topbar() -> Style {
    Style::default().fg(MUTED).bg(TOPBAR)
}

pub fn topbar_tab() -> Style {
    Style::default().fg(SUBTLE).bg(TOPBAR_DARK)
}

pub fn composer() -> Style {
    Style::default().fg(TEXT).bg(COMPOSER)
}

pub fn border(active: bool) -> Style {
    Style::default()
        .fg(if active { ACCENT } else { BORDER })
        .bg(PANEL)
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

pub fn unread() -> Style {
    Style::default()
        .fg(WARN)
        .bg(PANEL)
        .add_modifier(Modifier::BOLD)
}

pub fn message_author(is_current_user: bool) -> Style {
    Style::default()
        .fg(if is_current_user { ACCENT_SOFT } else { ACCENT })
        .bg(CARD)
        .add_modifier(Modifier::BOLD)
}

pub fn message_meta() -> Style {
    Style::default().fg(MUTED).bg(CARD)
}

pub fn message_body() -> Style {
    Style::default().fg(TEXT).bg(CARD)
}

pub fn message_card() -> Style {
    Style::default().fg(TEXT).bg(CARD)
}
