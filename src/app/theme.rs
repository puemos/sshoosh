use std::cell::Cell;

use ratatui::style::{Color, Modifier, Style};

use crate::{color::rgb_to_xterm256, terminal::ColorMode};

thread_local! {
    static COLOR_MODE: Cell<ColorMode> = const { Cell::new(ColorMode::Truecolor) };
}

// Dark palette adapted from bensimms.moe for terminal UI roles.
pub const BG: Color = Color::Rgb(34, 37, 41); // #222529
pub const COMPOSER: Color = Color::Rgb(41, 44, 47); // #292C2F
pub const KEYCAP: Color = Color::Rgb(52, 56, 59); // #34383B
pub const BADGE: Color = Color::Rgb(36, 39, 42); // #24272A
pub const PANEL: Color = BG;
pub const ELEVATED_PANEL: Color = COMPOSER;
pub const CARD: Color = BG;
pub const BORDER: Color = Color::Rgb(70, 73, 73); // #464949
pub const MESSAGE_SEPARATOR: Color = Color::Rgb(56, 60, 62); // #383C3E
pub const MUTED: Color = Color::Rgb(133, 146, 137); // #859289
pub const TEXT: Color = Color::Rgb(214, 214, 214); // #D6D6D6
pub const SUBTLE: Color = Color::Rgb(219, 213, 188); // #DBD5BC
pub const ACCENT: Color = Color::Rgb(120, 182, 173); // #78B6AD
pub const ACCENT_SOFT: Color = Color::Rgb(135, 201, 229); // #87C9E5
pub const MENTION: Color = Color::Rgb(232, 121, 211); // hot magenta — kept distinct from author palette
pub const PIN: Color = Color::Rgb(231, 224, 154); // #E7E09A
pub const SAVED: Color = Color::Rgb(154, 231, 178); // green marker

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
    color(AUTHOR_PALETTE[(index * 11) % AUTHOR_PALETTE.len()])
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
    color(AUTHOR_PALETTE[(hash as usize) % AUTHOR_PALETTE.len()])
}
pub const WARN: Color = Color::Rgb(226, 174, 162); // #E2AEA2
pub const OK: Color = ACCENT;
pub const ERROR: Color = Color::Rgb(230, 126, 128); // #E67E80

pub fn with_color_mode<T>(color_mode: ColorMode, render: impl FnOnce() -> T) -> T {
    let previous = set_color_mode(color_mode);
    let _reset = ColorModeReset(previous);
    render()
}

pub fn color(color: Color) -> Color {
    match (current_color_mode(), color) {
        (ColorMode::Ansi256, Color::Rgb(r, g, b)) => Color::Indexed(rgb_to_xterm256((r, g, b))),
        _ => color,
    }
}

pub fn bg() -> Color {
    color(BG)
}

pub fn composer_bg() -> Color {
    color(COMPOSER)
}

pub fn keycap() -> Color {
    color(KEYCAP)
}

pub fn badge() -> Color {
    color(BADGE)
}

pub fn panel_bg() -> Color {
    color(PANEL)
}

pub fn elevated_panel_bg() -> Color {
    color(ELEVATED_PANEL)
}

pub fn card_bg() -> Color {
    color(CARD)
}

pub fn border() -> Color {
    color(BORDER)
}

pub fn message_separator_color() -> Color {
    color(MESSAGE_SEPARATOR)
}

pub fn muted_color() -> Color {
    color(MUTED)
}

pub fn text_color() -> Color {
    color(TEXT)
}

pub fn subtle_color() -> Color {
    color(SUBTLE)
}

pub fn accent_color() -> Color {
    color(ACCENT)
}

pub fn accent_soft_color() -> Color {
    color(ACCENT_SOFT)
}

pub fn mention_color() -> Color {
    color(MENTION)
}

pub fn pin_color() -> Color {
    color(PIN)
}

pub fn saved_color() -> Color {
    color(SAVED)
}

pub fn warn_color() -> Color {
    color(WARN)
}

pub fn ok_color() -> Color {
    color(OK)
}

pub fn error_color() -> Color {
    color(ERROR)
}

pub fn base() -> Style {
    Style::default().fg(text_color()).bg(bg())
}

pub fn panel() -> Style {
    Style::default().fg(text_color()).bg(panel_bg())
}

pub fn elevated_panel() -> Style {
    Style::default().fg(text_color()).bg(elevated_panel_bg())
}

pub fn composer() -> Style {
    Style::default().fg(text_color()).bg(composer_bg())
}

pub fn title() -> Style {
    Style::default()
        .fg(text_color())
        .bg(panel_bg())
        .add_modifier(Modifier::BOLD)
}

pub fn muted() -> Style {
    Style::default().fg(muted_color()).bg(panel_bg())
}

pub fn elevated_muted() -> Style {
    Style::default().fg(muted_color()).bg(elevated_panel_bg())
}

pub fn section_header(active: bool) -> Style {
    Style::default()
        .fg(if active {
            accent_color()
        } else {
            subtle_color()
        })
        .bg(panel_bg())
        .add_modifier(Modifier::BOLD)
}

pub fn accent() -> Style {
    Style::default()
        .fg(accent_color())
        .bg(panel_bg())
        .add_modifier(Modifier::BOLD)
}

pub fn elevated_accent() -> Style {
    Style::default()
        .fg(accent_color())
        .bg(elevated_panel_bg())
        .add_modifier(Modifier::BOLD)
}

pub fn unread() -> Style {
    Style::default()
        .fg(warn_color())
        .bg(panel_bg())
        .add_modifier(Modifier::BOLD)
}

pub fn pin() -> Style {
    Style::default().fg(pin_color()).bg(panel_bg())
}

pub fn elevated_title() -> Style {
    Style::default()
        .fg(text_color())
        .bg(elevated_panel_bg())
        .add_modifier(Modifier::BOLD)
}

pub fn elevated_unread() -> Style {
    Style::default()
        .fg(warn_color())
        .bg(elevated_panel_bg())
        .add_modifier(Modifier::BOLD)
}

pub fn selection() -> Style {
    Style::default().fg(bg()).bg(accent_color())
}

pub fn strong_selection() -> Style {
    selection().add_modifier(Modifier::BOLD)
}

pub fn message_author_on(author_color: Color, bg: Color) -> Style {
    Style::default()
        .fg(color(author_color))
        .bg(color(bg))
        .add_modifier(Modifier::BOLD)
}

pub fn message_meta_on(bg: Color) -> Style {
    Style::default().fg(muted_color()).bg(color(bg))
}

pub fn message_saved_on(bg: Color) -> Style {
    Style::default()
        .fg(saved_color())
        .bg(color(bg))
        .add_modifier(Modifier::BOLD)
}

pub fn message_body() -> Style {
    Style::default().fg(text_color()).bg(card_bg())
}

pub fn message_mention() -> Style {
    Style::default()
        .fg(mention_color())
        .bg(card_bg())
        .add_modifier(Modifier::BOLD)
}

pub fn message_label() -> Style {
    Style::default()
        .fg(accent_color())
        .bg(card_bg())
        .add_modifier(Modifier::BOLD)
}

pub fn message_link() -> Style {
    Style::default()
        .fg(accent_soft_color())
        .bg(card_bg())
        .add_modifier(Modifier::UNDERLINED)
}

pub fn message_link_target() -> Style {
    Style::default().fg(muted_color()).bg(card_bg())
}

pub fn message_code() -> Style {
    Style::default().fg(subtle_color()).bg(card_bg())
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
    Style::default().fg(text_color()).bg(color(bg))
}

pub fn reaction_chip(active: bool) -> Style {
    let fg = if active {
        accent_soft_color()
    } else {
        text_color()
    };
    Style::default()
        .fg(fg)
        .bg(keycap())
        .add_modifier(Modifier::BOLD)
}

pub fn reaction_add_chip() -> Style {
    Style::default()
        .fg(muted_color())
        .bg(keycap())
        .add_modifier(Modifier::BOLD)
}

pub fn message_separator() -> Style {
    Style::default()
        .fg(message_separator_color())
        .bg(panel_bg())
}

fn current_color_mode() -> ColorMode {
    COLOR_MODE.with(Cell::get)
}

fn set_color_mode(color_mode: ColorMode) -> ColorMode {
    COLOR_MODE.with(|cell| {
        let previous = cell.get();
        cell.set(color_mode);
        previous
    })
}

struct ColorModeReset(ColorMode);

impl Drop for ColorModeReset {
    fn drop(&mut self) {
        set_color_mode(self.0);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn truecolor_mode_keeps_canonical_rgb_palette() {
        with_color_mode(ColorMode::Truecolor, || {
            assert_eq!(bg(), BG);
            assert_eq!(composer_bg(), COMPOSER);
            assert_eq!(badge(), BADGE);
            assert_eq!(base().bg, Some(BG));
        });
    }

    #[test]
    fn ansi256_mode_reduces_rgb_palette_to_indexed_colors() {
        with_color_mode(ColorMode::Ansi256, || {
            assert_eq!(bg(), Color::Indexed(rgb_to_xterm256((34, 37, 41))));
            assert_eq!(composer_bg(), Color::Indexed(rgb_to_xterm256((41, 44, 47))));
            assert_eq!(badge(), Color::Indexed(rgb_to_xterm256((36, 39, 42))));
            assert!(matches!(base().bg, Some(Color::Indexed(_))));
            assert!(matches!(base().fg, Some(Color::Indexed(_))));
        });
    }

    #[test]
    fn color_mode_scope_resets_after_render() {
        with_color_mode(ColorMode::Ansi256, || {
            assert!(matches!(text_color(), Color::Indexed(_)));
        });

        assert_eq!(text_color(), TEXT);
    }
}
