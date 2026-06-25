use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};

use crate::theme::BRAND;

const BRAND_WORDMARK: &str = r#"██    ██ ██ ██████  ███████  █████  ██████   ██████  ██    ██ ███    ██ ██████
██    ██ ██ ██   ██ ██      ██   ██ ██   ██ ██    ██ ██    ██ ████   ██ ██   ██
██    ██ ██ ██████  █████   ███████ ██████  ██    ██ ██    ██ ██ ██  ██ ██   ██
 ██  ██  ██ ██   ██ ██      ██   ██ ██   ██ ██    ██ ██    ██ ██  ██ ██ ██   ██
  ████   ██ ██████  ███████ ██   ██ ██   ██  ██████   ██████  ██   ████ ██████"#;
const BRAND_MARK: &str = r#"██   ██   ████
██   ██  ██  ██
 ██ ██   ██████
  ███    ██  ██"#;

/// Minimum width at which the full wordmark fits; below this the compact "VA"
/// mark is used instead.
const FULL_WORDMARK_MIN_WIDTH: u16 = 80;

/// Block width of the compact "VA" mark, for laying out the working header.
pub(super) const MARK_WIDTH: u16 = 15;

pub(super) const VERSION: &str = concat!("v", env!("CARGO_PKG_VERSION"));

/// The brand wordmark as block art, sized to the available width. Used as the
/// hero on the welcome screen.
pub(super) fn wordmark_lines(content_width: u16) -> Vec<Line<'static>> {
    let art = if content_width >= FULL_WORDMARK_MIN_WIDTH {
        BRAND_WORDMARK
    } else {
        BRAND_MARK
    };
    art_block(art)
}

/// The compact "VA" mark, always — the brand corner of the working header.
pub(super) fn mark_lines() -> Vec<Line<'static>> {
    art_block(BRAND_MARK)
}

/// Render block art in the brand color, every row padded to a shared width so
/// the block stays rectangular.
fn art_block(art: &'static str) -> Vec<Line<'static>> {
    let art_lines = art.lines().collect::<Vec<_>>();
    let widths = art_lines
        .iter()
        .map(|line| Line::from((*line).to_string()).width())
        .collect::<Vec<_>>();
    let block_width = widths.iter().copied().max().unwrap_or(0);
    let art_style = Style::default().fg(BRAND).add_modifier(Modifier::BOLD);

    art_lines
        .into_iter()
        .zip(widths)
        .map(|(line, width)| {
            Line::from(Span::styled(
                format!("{line}{}", " ".repeat(block_width.saturating_sub(width))),
                art_style,
            ))
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wordmark_rows_share_one_block_width() {
        let lines = wordmark_lines(120);
        let widths = lines.iter().map(Line::width).collect::<Vec<_>>();

        assert_eq!(lines.len(), BRAND_WORDMARK.lines().count());
        assert!(widths.iter().all(|width| *width == widths[0]));
    }

    #[test]
    fn mark_width_matches_rendered_block() {
        let lines = mark_lines();
        assert!(lines
            .iter()
            .all(|line| line.width() == usize::from(MARK_WIDTH)));
    }

    #[test]
    fn narrow_width_uses_compact_mark() {
        let lines = wordmark_lines(40);
        let rendered = lines
            .iter()
            .map(|line| {
                line.spans
                    .iter()
                    .map(|span| span.content.as_ref())
                    .collect::<String>()
            })
            .collect::<Vec<_>>()
            .join("\n");

        assert!(rendered.contains('█'));
        assert!(!rendered.contains("VibeAround"));
        assert_eq!(lines.len(), BRAND_MARK.lines().count());
    }
}
