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
pub const MESSAGE_CARD_FOCUSED: Color = Color::Rgb(29, 32, 34); // #1D2022
pub const BORDER: Color = Color::Rgb(70, 73, 73); // #464949
pub const MESSAGE_SEPARATOR: Color = Color::Rgb(56, 60, 62); // #383C3E
pub const MUTED: Color = Color::Rgb(133, 146, 137); // #859289
pub const TEXT: Color = Color::Rgb(214, 214, 214); // #D6D6D6
pub const SUBTLE: Color = Color::Rgb(219, 213, 188); // #DBD5BC
pub const ACCENT: Color = Color::Rgb(120, 182, 173); // #78B6AD
pub const ACCENT_SOFT: Color = Color::Rgb(135, 201, 229); // #87C9E5
pub const MENTION: Color = Color::Rgb(232, 121, 211); // hot magenta — kept distinct from author palette
pub const MESSAGE_ERROR_GUTTER: Color = Color::Rgb(230, 126, 128); // #E67E80

// Stable per-author palette: 16 hues evenly spaced around the color wheel,
// at fixed saturation/lightness tuned for the dark message surface.
const AUTHOR_PALETTE: &[Color] = &[
    Color::Rgb(231, 154, 154), //   0°  salmon
    Color::Rgb(231, 178, 154), //  22°  peach
    Color::Rgb(231, 202, 154), //  45°  apricot
    Color::Rgb(231, 224, 154), //  67°  butter
    Color::Rgb(208, 231, 154), //  90°  pear
    Color::Rgb(184, 231, 154), // 112°  spring
    Color::Rgb(160, 231, 154), // 135°  leaf
    Color::Rgb(154, 231, 178), // 157°  emerald
    Color::Rgb(154, 231, 208), // 180°  mint
    Color::Rgb(154, 220, 231), // 202°  aqua
    Color::Rgb(154, 196, 231), // 225°  sky
    Color::Rgb(154, 172, 231), // 247°  blue
    Color::Rgb(178, 154, 231), // 270°  lavender
    Color::Rgb(208, 154, 231), // 292°  violet
    Color::Rgb(231, 154, 220), // 315°  magenta
    Color::Rgb(231, 154, 184), // 337°  pink
];

/// Maps a user's index (their position in the sorted user list) to a palette
/// color. Multiplied by a coprime of palette length so consecutively-sorted
/// users land on far-apart hues rather than adjacent slots.
pub fn author_color_for_index(index: usize) -> Color {
    AUTHOR_PALETTE[(index * 11) % AUTHOR_PALETTE.len()]
}

/// Fallback when an author is not present in the user list (e.g. system
/// accounts, deleted users). djb2 hash → palette slot.
pub fn author_color_fallback(author: &str) -> Color {
    let mut hash: u32 = 5381;
    for byte in author.as_bytes() {
        hash = hash
            .wrapping_mul(33)
            .wrapping_add(byte.to_ascii_lowercase() as u32);
    }
    AUTHOR_PALETTE[(hash as usize) % AUTHOR_PALETTE.len()]
}
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

pub fn message_author_on(author_color: Color, bg: Color) -> Style {
    Style::default()
        .fg(author_color)
        .bg(bg)
        .add_modifier(Modifier::BOLD)
}

pub fn message_meta_on(bg: Color) -> Style {
    Style::default().fg(MUTED).bg(bg)
}

pub fn message_body() -> Style {
    Style::default().fg(TEXT).bg(CARD)
}

pub fn message_mention() -> Style {
    Style::default()
        .fg(MENTION)
        .bg(CARD)
        .add_modifier(Modifier::BOLD)
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
