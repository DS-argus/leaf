use crate::markdown::LinkSpan;
use ratatui::{
    buffer::{Buffer, CellWidth},
    layout::Rect,
    style::{Color, Style},
    text::{Line, Span},
    widgets::{Paragraph, Widget, Wrap},
};
use std::collections::HashMap;

#[derive(Clone, Debug)]
pub(crate) struct ScreenLink {
    pub(crate) row: usize,
    pub(crate) start_col: usize,
    pub(crate) end_col: usize,
    pub(crate) span_idx: usize,
}

#[derive(Clone, Debug, Default)]
pub(crate) struct ScreenGeometry {
    pub(crate) row_lines: Vec<usize>,
    pub(crate) links: Vec<ScreenLink>,
}

pub(crate) fn screen_geometry(
    lines: &[Line<'_>],
    spans_by_line: &HashMap<usize, Vec<LinkSpan>>,
    first_line: usize,
    gutter_width: usize,
    width: u16,
    height: u16,
) -> ScreenGeometry {
    let mut geometry = ScreenGeometry::default();
    if width == 0 || height == 0 {
        return geometry;
    }

    let mut probes = Vec::new();
    for (offset, line) in lines.iter().enumerate() {
        let remaining = usize::from(height) - geometry.row_lines.len();
        if remaining == 0 {
            break;
        }
        let line_idx = first_line + offset;
        let ranges = spans_by_line
            .get(&line_idx)
            .map(Vec::as_slice)
            .unwrap_or(&[]);
        let probe = probe_line(line, ranges, gutter_width);
        let rows = wrapped(vec![probe.clone()]).line_count(width);
        geometry
            .row_lines
            .extend(std::iter::repeat_n(line_idx, rows.min(remaining)));
        probes.push(probe);
    }

    let area = Rect::new(0, 0, width, height);
    let slack = super::CONTENT_HORIZONTAL_PADDING + super::SCROLLBAR_WIDTH;
    let mut buffer = Buffer::empty(Rect::new(0, 0, width.saturating_add(slack), height));
    wrapped(probes).render(area, &mut buffer);

    for row in 0..geometry.row_lines.len() {
        let mut col = 0;
        while col < width {
            let cell = &buffer[(col, row as u16)];
            let end = width.min(col + cell.symbol().cell_width().max(1));
            if let Some(span_idx) = link_index(cell.fg) {
                let (start_col, end_col) = (usize::from(col), usize::from(end));
                match geometry.links.last_mut() {
                    Some(link)
                        if link.row == row
                            && link.span_idx == span_idx
                            && link.end_col == start_col =>
                    {
                        link.end_col = end_col;
                    }
                    _ => geometry.links.push(ScreenLink {
                        row,
                        start_col,
                        end_col,
                        span_idx,
                    }),
                }
            }
            col = end;
        }
    }
    geometry
}

fn wrapped<'a>(lines: Vec<Line<'a>>) -> Paragraph<'a> {
    Paragraph::new(lines).wrap(Wrap { trim: false })
}

fn probe_line<'a>(line: &'a Line<'_>, ranges: &[LinkSpan], gutter_width: usize) -> Line<'a> {
    let mut col = 0;
    let spans: Vec<Span<'a>> = line
        .styled_graphemes(Style::default())
        .map(|grapheme| {
            let start = col;
            col += grapheme.symbol.cell_width() as usize;
            let owner = ranges.iter().position(|range| {
                range.start_col + gutter_width <= start && start < range.end_col + gutter_width
            });
            let style = owner.map_or_else(Style::default, |index| {
                Style::default().fg(link_color(index))
            });
            Span::styled(grapheme.symbol, style)
        })
        .collect();
    let probe = Line::from(spans);
    match line.alignment {
        Some(alignment) => probe.alignment(alignment),
        None => probe,
    }
}

fn link_color(index: usize) -> Color {
    Color::Rgb((index >> 16) as u8, (index >> 8) as u8, index as u8)
}

fn link_index(color: Color) -> Option<usize> {
    match color {
        Color::Rgb(r, g, b) => Some(usize::from(r) << 16 | usize::from(g) << 8 | usize::from(b)),
        _ => None,
    }
}
