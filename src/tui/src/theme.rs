use ratatui::style::{Color, Modifier, Style};

pub(crate) const BRAND: Color = Color::Rgb(79, 209, 197);
pub(crate) const OK: Color = Color::Reset;
pub(crate) const WARN: Color = Color::Yellow;
pub(crate) const ERROR: Color = Color::Red;
pub(crate) const NEUTRAL: Color = Color::Reset;

pub(crate) fn muted_style() -> Style {
    Style::default().add_modifier(Modifier::DIM)
}
