use ratatui::style::{Color, Modifier, Style};
use ratatui::symbols::border;

pub(crate) const BRAND: Color = Color::Rgb(79, 209, 197);
pub(crate) const INPUT_BG: Color = Color::Rgb(245, 248, 248);
pub(crate) const OK: Color = Color::Reset;
pub(crate) const WARN: Color = Color::Yellow;
pub(crate) const ERROR: Color = Color::Red;
pub(crate) const NEUTRAL: Color = Color::Reset;
pub(crate) const SEMANTIC_BORDER: border::Set = border::Set {
    top_left: " ",
    top_right: " ",
    bottom_left: " ",
    bottom_right: " ",
    vertical_left: "│",
    vertical_right: "│",
    horizontal_top: "─",
    horizontal_bottom: "─",
};

pub(crate) fn muted_style() -> Style {
    Style::default().add_modifier(Modifier::DIM)
}
