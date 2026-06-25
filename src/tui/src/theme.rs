use ratatui::style::{Color, Modifier, Style};
use ratatui::symbols::border;

/// The single hard-coded color in the dashboard: VibeAround's brand teal.
///
/// It mirrors the `--primary` design token shared by the desktop and web
/// apps (`oklch(0.72 0.15 180)` ≈ `#00C2A8`) so the terminal carries the
/// same identity. Everything else defers to the user's terminal theme via
/// ANSI / default colors, so the dashboard adapts to whatever palette they
/// have configured instead of fighting it.
pub(crate) const BRAND: Color = Color::Rgb(0, 194, 168);

/// Accent for focus, selection, keybindings, and prompts. Aliased to
/// [`BRAND`] so the brand color is carried through the whole UI rather
/// than a separate, generic cyan.
pub(crate) const ACTION: Color = BRAND;

/// Status colors resolve against the terminal's themed ANSI palette.
pub(crate) const OK: Color = Color::Green;
pub(crate) const WARN: Color = Color::Yellow;
pub(crate) const ERROR: Color = Color::Red;
pub(crate) const NEUTRAL: Color = Color::Gray;

/// A faint "surface" tint that lifts the chat input off the canvas as a
/// slim bar (à la Claude Code). This is the one neutral we set beyond the
/// brand color; index 254 is a near-white gray that reads as a subtle
/// highlight on light terminals.
pub(crate) const INPUT_BG: Color = Color::Indexed(254);
/// User messages stay on the terminal's own background — no fixed RGB.
pub(crate) const REQUEST_BG: Color = Color::Reset;

/// A light hairline used only to underline the brand bar. No corners or
/// boxes — focus is communicated with the brand accent, not chrome.
pub(crate) const SEMANTIC_BORDER: border::Set = border::Set {
    top_left: " ",
    top_right: " ",
    bottom_left: " ",
    bottom_right: " ",
    vertical_left: "▏",
    vertical_right: " ",
    horizontal_top: "─",
    horizontal_bottom: "─",
};

/// A thick brand-colored bar drawn down the left edge of the input — the
/// accent that frames the prompt as a deliberate input zone.
pub(crate) const INPUT_ACCENT: border::Set = border::Set {
    top_left: "▌",
    top_right: " ",
    bottom_left: "▌",
    bottom_right: " ",
    vertical_left: "▌",
    vertical_right: " ",
    horizontal_top: " ",
    horizontal_bottom: " ",
};

pub(crate) fn muted_style() -> Style {
    Style::default().add_modifier(Modifier::DIM)
}

/// Bold text in the brand accent — the primary emphasis treatment.
pub(crate) fn accent_style() -> Style {
    Style::default().fg(ACTION).add_modifier(Modifier::BOLD)
}
