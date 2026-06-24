use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::Paragraph;

use crate::app::TuiApp;
use crate::theme::{muted_style, ACTION, BRAND};

const BRAND_WORDMARK: &str = r#"██    ██ ██ ██████  ███████  █████  ██████   ██████  ██    ██ ███    ██ ██████
██    ██ ██ ██   ██ ██      ██   ██ ██   ██ ██    ██ ██    ██ ████   ██ ██   ██
██    ██ ██ ██████  █████   ███████ ██████  ██    ██ ██    ██ ██ ██  ██ ██   ██
 ██  ██  ██ ██   ██ ██      ██   ██ ██   ██ ██    ██ ██    ██ ██  ██ ██ ██   ██
  ████   ██ ██████  ███████ ██   ██ ██   ██  ██████   ██████  ██   ████ ██████"#;
const BRAND_MARK: &str = r#"██    ██  █████
██    ██ ██   ██
██    ██ ███████
 ██  ██  ██   ██
  ████   ██   ██"#;
const TAGLINE: &str = "unified runtime for ai coding agents";
const VERSION: &str = concat!("v", env!("CARGO_PKG_VERSION"));

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum BrandMode {
    Mark,
    Full,
}

impl BrandMode {
    pub(super) fn height(self) -> u16 {
        match self {
            Self::Mark => 7,
            Self::Full => 8,
        }
    }
}

pub(super) fn brand_mode(width: u16, height: u16) -> BrandMode {
    if width >= 92 && height >= 16 {
        BrandMode::Full
    } else {
        BrandMode::Mark
    }
}

pub(super) fn brand_header(
    app: &TuiApp,
    mode: BrandMode,
    content_width: u16,
) -> Paragraph<'static> {
    let content_width = usize::from(content_width);
    let mut lines = Vec::new();
    match mode {
        BrandMode::Full => {
            lines.push(Line::raw(""));
            lines.extend(padded_art_lines(BRAND_WORDMARK, content_width));
            lines.push(Line::raw(""));
            lines.push(metadata_line(&app.endpoint));
        }
        BrandMode::Mark => {
            lines.push(Line::raw(""));
            lines.extend(padded_art_lines(BRAND_MARK, content_width));
            lines.push(Line::raw(""));
        }
    }

    Paragraph::new(lines)
}

fn padded_art_lines(art: &'static str, _content_width: usize) -> Vec<Line<'static>> {
    let art_lines = art.lines().collect::<Vec<_>>();
    let widths = art_lines
        .iter()
        .map(|line| Line::from((*line).to_string()).width())
        .collect::<Vec<_>>();
    let block_width = widths.iter().copied().max().unwrap_or(0);
    let left_pad = 0;

    art_lines
        .into_iter()
        .zip(widths)
        .map(|(line, width)| {
            Line::from(Span::styled(
                format!(
                    "{}{}{}",
                    " ".repeat(left_pad),
                    line,
                    " ".repeat(block_width.saturating_sub(width))
                ),
                Style::default().fg(BRAND).add_modifier(Modifier::BOLD),
            ))
        })
        .collect()
}

fn metadata_line(endpoint: &str) -> Line<'static> {
    Line::from(vec![
        Span::styled(TAGLINE, muted_style().add_modifier(Modifier::BOLD)),
        Span::styled("   /   ", muted_style()),
        Span::styled(endpoint.to_string(), Style::default().fg(ACTION)),
        Span::styled("   /   ", muted_style()),
        Span::styled(VERSION, muted_style()),
    ])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn brand_mode_scales_with_terminal_size() {
        assert_eq!(brand_mode(80, 24), BrandMode::Mark);
        assert_eq!(brand_mode(92, 15), BrandMode::Mark);
        assert_eq!(brand_mode(92, 16), BrandMode::Full);
        assert_eq!(brand_mode(120, 24), BrandMode::Full);
        assert_eq!(BrandMode::Mark.height(), 7);
        assert_eq!(BrandMode::Full.height(), 8);
    }

    #[test]
    fn padded_art_lines_share_one_block_width() {
        let lines = padded_art_lines(BRAND_WORDMARK, 120);
        let widths = lines.iter().map(Line::width).collect::<Vec<_>>();

        assert_eq!(lines.len(), BRAND_WORDMARK.lines().count());
        assert!(widths.iter().all(|width| *width == widths[0]));
        assert!(widths[0] <= 120);
    }

    #[test]
    fn compact_brand_is_art_not_plain_text() {
        let lines = padded_art_lines(BRAND_MARK, 20);
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

        assert!(!rendered.contains("VA"));
        assert!(!rendered.contains("VibeAround"));
        assert!(rendered.contains('█'));
    }
}
