use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::Paragraph;

use crate::app::TuiApp;
use crate::theme::{muted_style, BRAND};

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
    Wordmark,
    WordmarkWithMeta,
}

impl BrandMode {
    pub(super) fn height(self) -> u16 {
        match self {
            Self::Mark | Self::Wordmark => 5,
            Self::WordmarkWithMeta => 6,
        }
    }
}

pub(super) fn brand_mode(width: u16, height: u16) -> BrandMode {
    if width >= 88 && height >= 18 {
        BrandMode::WordmarkWithMeta
    } else if width >= 84 && height >= 12 {
        BrandMode::Wordmark
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
        BrandMode::WordmarkWithMeta => {
            lines.extend(padded_art_lines(BRAND_WORDMARK, content_width));
            lines.push(metadata_line(content_width, &app.endpoint));
        }
        BrandMode::Wordmark => {
            lines.extend(padded_art_lines(BRAND_WORDMARK, content_width));
        }
        BrandMode::Mark => {
            lines.extend(padded_art_lines(BRAND_MARK, content_width));
        }
    }

    Paragraph::new(lines)
}

fn padded_art_lines(art: &'static str, content_width: usize) -> Vec<Line<'static>> {
    let art_lines = art.lines().collect::<Vec<_>>();
    let widths = art_lines
        .iter()
        .map(|line| Line::from((*line).to_string()).width())
        .collect::<Vec<_>>();
    let block_width = widths.iter().copied().max().unwrap_or(0);
    let left_pad = art_left_pad(content_width, block_width);

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

fn metadata_line(content_width: usize, endpoint: &str) -> Line<'static> {
    padded_line(
        content_width,
        vec![
            Span::styled(TAGLINE, muted_style().add_modifier(Modifier::BOLD)),
            Span::styled("   /   ", muted_style()),
            Span::styled(endpoint.to_string(), Style::default().fg(BRAND)),
            Span::styled("   /   ", muted_style()),
            Span::styled(VERSION, muted_style()),
        ],
    )
}

fn padded_line(content_width: usize, spans: Vec<Span<'static>>) -> Line<'static> {
    let line_width = Line::from(spans.clone()).width();
    let left_pad = text_left_pad(content_width, line_width);
    let mut padded_spans = Vec::with_capacity(spans.len() + 1);
    if left_pad > 0 {
        padded_spans.push(Span::raw(" ".repeat(left_pad)));
    }
    padded_spans.extend(spans);
    Line::from(padded_spans)
}

fn art_left_pad(content_width: usize, block_width: usize) -> usize {
    if content_width <= block_width {
        0
    } else if content_width >= 96 {
        4
    } else {
        2
    }
}

fn text_left_pad(content_width: usize, line_width: usize) -> usize {
    if content_width <= line_width {
        0
    } else if content_width >= 96 {
        4
    } else {
        2
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn brand_mode_scales_with_terminal_size() {
        assert_eq!(brand_mode(40, 24), BrandMode::Mark);
        assert_eq!(brand_mode(80, 18), BrandMode::Mark);
        assert_eq!(brand_mode(84, 17), BrandMode::Wordmark);
        assert_eq!(brand_mode(88, 18), BrandMode::WordmarkWithMeta);
        assert_eq!(BrandMode::Mark.height(), 5);
        assert_eq!(BrandMode::Wordmark.height(), 5);
        assert_eq!(BrandMode::WordmarkWithMeta.height(), 6);
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
