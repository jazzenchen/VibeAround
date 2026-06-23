use ratatui::layout::Alignment;
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::Paragraph;

use crate::app::TuiApp;
use crate::theme::{muted_style, BRAND};

const BRAND_LOGO: &str = r#" ██╗   ██╗ ██╗ ██████╗  ███████╗  █████╗  ██████╗   ██████╗  ██╗   ██╗ ███╗   ██╗ ██████╗
 ██║   ██║ ██║ ██╔══██╗ ██╔════╝ ██╔══██╗ ██╔══██╗ ██╔═══██╗ ██║   ██║ ████╗  ██║ ██╔══██╗
 ██║   ██║ ██║ ██████╔╝ █████╗   ███████║ ██████╔╝ ██║   ██║ ██║   ██║ ██╔██╗ ██║ ██║  ██║
 ╚██╗ ██╔╝ ██║ ██╔══██╗ ██╔══╝   ██╔══██║ ██╔══██╗ ██║   ██║ ██║   ██║ ██║╚██╗██║ ██║  ██║
  ╚████╔╝  ██║ ██████╔╝ ███████╗ ██║  ██║ ██║  ██║ ╚██████╔╝ ╚██████╔╝ ██║ ╚████║ ██████╔╝
   ╚═══╝   ╚═╝ ╚═════╝  ╚══════╝ ╚═╝  ╚═╝ ╚═╝  ╚═╝  ╚═════╝   ╚═════╝  ╚═╝  ╚═══╝ ╚═════╝"#;
const TAGLINE: &str = "unified runtime for ai coding agents";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum BrandMode {
    Narrow,
    Compact,
    FullLogo,
}

impl BrandMode {
    pub(super) fn height(self) -> u16 {
        match self {
            Self::Narrow => 3,
            Self::Compact => 4,
            Self::FullLogo => 9,
        }
    }
}

pub(super) fn brand_mode(width: u16, height: u16) -> BrandMode {
    if width >= 96 && height >= 24 {
        BrandMode::FullLogo
    } else if width >= 56 && height >= 14 {
        BrandMode::Compact
    } else {
        BrandMode::Narrow
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
        BrandMode::FullLogo => {
            lines.extend(centered_brand_logo_lines(content_width));
            lines.push(centered_line(
                content_width,
                vec![
                    Span::styled(TAGLINE, muted_style().add_modifier(Modifier::BOLD)),
                    Span::styled("   /   ", muted_style()),
                    Span::raw(app.endpoint.clone()),
                ],
            ));
        }
        BrandMode::Compact => {
            lines.push(centered_line(
                content_width,
                vec![
                    Span::styled(
                        "VibeAround",
                        Style::default().fg(BRAND).add_modifier(Modifier::BOLD),
                    ),
                    Span::styled("  terminal runtime console", muted_style()),
                ],
            ));
            lines.push(centered_line(
                content_width,
                vec![Span::raw(app.endpoint.clone())],
            ));
        }
        BrandMode::Narrow => {
            lines.push(centered_line(
                content_width,
                vec![Span::styled(
                    "VA",
                    Style::default().fg(BRAND).add_modifier(Modifier::BOLD),
                )],
            ));
        }
    }

    Paragraph::new(lines).alignment(Alignment::Center)
}

fn centered_brand_logo_lines(content_width: usize) -> Vec<Line<'static>> {
    let logo_lines = BRAND_LOGO.lines().collect::<Vec<_>>();
    let widths = logo_lines
        .iter()
        .map(|line| Line::from((*line).to_string()).width())
        .collect::<Vec<_>>();
    let block_width = widths.iter().copied().max().unwrap_or(0);
    let left_pad = content_width.saturating_sub(block_width) / 2;

    logo_lines
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

fn centered_line(content_width: usize, spans: Vec<Span<'static>>) -> Line<'static> {
    let line_width = Line::from(spans.clone()).width();
    let left_pad = content_width.saturating_sub(line_width) / 2;
    let mut padded_spans = Vec::with_capacity(spans.len() + 1);
    if left_pad > 0 {
        padded_spans.push(Span::raw(" ".repeat(left_pad)));
    }
    padded_spans.extend(spans);
    Line::from(padded_spans)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn brand_mode_scales_with_terminal_size() {
        assert_eq!(brand_mode(40, 24), BrandMode::Narrow);
        assert_eq!(brand_mode(80, 18), BrandMode::Compact);
        assert_eq!(brand_mode(96, 24), BrandMode::FullLogo);
        assert_eq!(BrandMode::Narrow.height(), 3);
        assert_eq!(BrandMode::FullLogo.height(), 9);
    }

    #[test]
    fn centered_brand_logo_lines_share_one_block_width() {
        let lines = centered_brand_logo_lines(120);
        let widths = lines.iter().map(Line::width).collect::<Vec<_>>();

        assert_eq!(lines.len(), BRAND_LOGO.lines().count());
        assert!(widths.iter().all(|width| *width == widths[0]));
        assert!(widths[0] <= 120);
    }
}
