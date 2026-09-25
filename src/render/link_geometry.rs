//! Bounded projection of parsed [`Line`]s through Ratatui's wrapped paragraph geometry.
//!
//! `Paragraph` keeps the parsed lines as its input and performs a second, visual wrapping pass at
//! the terminal width. This module mirrors the locked Ratatui `WordWrapper` algorithm for
//! `Wrap { trim: false }`, but stores only the rows and cell ranges needed for one viewport.

// Wrapping algorithm adapted from ratatui-widgets 0.3.2, src/reflow.rs.
// Copyright (c) 2016-2022 Florian Dehau
// Copyright (c) 2023-2025 The Ratatui Developers
//
// Permission is hereby granted, free of charge, to any person obtaining a copy
// of this software and associated documentation files (the "Software"), to deal
// in the Software without restriction, including without limitation the rights
// to use, copy, modify, merge, publish, distribute, sublicense, and/or sell
// copies of the Software, and to permit persons to whom the Software is
// furnished to do so, subject to the following conditions:
//
// The above copyright notice and this permission notice shall be included in all
// copies or substantial portions of the Software.
//
// THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND, EXPRESS OR
// IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY,
// FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE
// AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER
// LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM,
// OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE
// SOFTWARE.
use ratatui::{buffer::CellWidth, layout::Alignment, style::Style, text::Line};
use std::collections::VecDeque;

/// A contiguous mapping between cells in one parsed line and one projected viewport row.
///
/// Both ranges are half-open and measured in terminal cells. `visual_row` is relative to the
/// projected viewport, while `logical_line` is the index in the input slice (and therefore remains
/// an absolute document index when the caller passes the full document).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct VisualSegment {
    pub(crate) logical_line: usize,
    pub(crate) logical_start_col: usize,
    pub(crate) logical_end_col: usize,
    pub(crate) visual_row: usize,
    pub(crate) visual_start_col: usize,
    pub(crate) visual_end_col: usize,
}

/// The row-level mapping emitted for one wrapped source line.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct VisualRow {
    pub(crate) visual_row: usize,
    pub(crate) logical_line: usize,
    pub(crate) logical_row: usize,
    pub(crate) logical_start_col: usize,
    pub(crate) logical_end_col: usize,
    pub(crate) visual_start_col: usize,
    pub(crate) visual_end_col: usize,
}

/// Cell mappings for the requested viewport only.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct ViewportProjection {
    pub(crate) rows: Vec<VisualRow>,
    pub(crate) segments: Vec<VisualSegment>,
}

impl ViewportProjection {
    /// Returns the visual segment containing a parsed-line cell, if that cell is visible.
    #[cfg(test)]
    pub(crate) fn visual_for_logical(
        &self,
        logical_line: usize,
        logical_col: usize,
    ) -> Option<&VisualSegment> {
        self.segments.iter().find(|segment| {
            segment.logical_line == logical_line
                && segment.logical_start_col <= logical_col
                && logical_col < segment.logical_end_col
        })
    }

    /// Returns the parsed line and cell range owning a visible terminal cell.
    pub(crate) fn logical_at_visual(
        &self,
        visual_row: usize,
        visual_col: usize,
    ) -> Option<(usize, usize)> {
        self.segments.iter().find_map(|segment| {
            (segment.visual_row == visual_row
                && segment.visual_start_col <= visual_col
                && visual_col < segment.visual_end_col)
                .then(|| {
                    (
                        segment.logical_line,
                        segment.logical_start_col + (visual_col - segment.visual_start_col),
                    )
                })
        })
    }

    /// Returns a row-level mapping for a viewport row.
    pub(crate) fn row(&self, visual_row: usize) -> Option<&VisualRow> {
        self.rows.get(visual_row)
    }
}

/// Projects `lines` through `Paragraph::wrap(Wrap { trim: false })` for one viewport.
///
/// `parsed_line_start` selects the first parsed line to inspect. `within_line_offset` skips that
/// many wrapped rows of the first selected line; it is the bounded intra-line offset needed for a
/// tall heading. The returned row numbers are viewport-relative, and at most `viewport_height`
/// rows and their cell segments are retained. A zero width or height produces an empty projection.
///
/// The implementation intentionally follows Ratatui 0.30.2's private `WordWrapper`: word
/// boundaries, preserved whitespace, zero-width graphemes, and graphemes wider than the target
/// width therefore have the same behavior as the Paragraph widget. Logical and visual columns use
/// Ratatui's terminal-cell width rather than byte or Unicode scalar offsets.
pub(crate) fn project_viewport(
    lines: &[Line<'_>],
    width: usize,
    viewport_height: usize,
    parsed_line_start: usize,
    within_line_offset: usize,
) -> ViewportProjection {
    let width = width.min(u16::MAX as usize);
    if width == 0 || viewport_height == 0 || parsed_line_start >= lines.len() {
        return ViewportProjection::default();
    }

    let mut projection = ViewportProjection::default();
    for (logical_line, line) in lines.iter().enumerate().skip(parsed_line_start) {
        let wrapped = wrap_line(line, width);
        let skipped = if logical_line == parsed_line_start {
            within_line_offset
        } else {
            0
        };

        for (logical_row, wrapped_row) in wrapped.into_iter().enumerate().skip(skipped) {
            if projection.rows.len() >= viewport_height {
                return projection;
            }

            let visual_row = projection.rows.len();
            let visual_start_col = line_offset(
                line.alignment.unwrap_or(Alignment::Left),
                width,
                wrapped_row.width,
            );
            let visual_end_col = (visual_start_col + wrapped_row.width).min(width);
            let mut visual_col = visual_start_col;
            let mut logical_start_col = usize::MAX;
            let mut logical_end_col = 0;

            for grapheme in wrapped_row.items {
                if grapheme.width == 0 {
                    continue;
                }
                if visual_col >= width {
                    break;
                }
                let visual_end = (visual_col + grapheme.width).min(width);
                let clipped_logical_end = grapheme.logical_start_col + visual_end - visual_col;
                logical_start_col = logical_start_col.min(grapheme.logical_start_col);
                logical_end_col = logical_end_col.max(clipped_logical_end);
                append_segment(
                    &mut projection.segments,
                    VisualSegment {
                        logical_line,
                        logical_start_col: grapheme.logical_start_col,
                        logical_end_col: clipped_logical_end,
                        visual_row,
                        visual_start_col: visual_col,
                        visual_end_col: visual_end,
                    },
                );
                visual_col = visual_end;
            }

            projection.rows.push(VisualRow {
                visual_row,
                logical_line,
                logical_row,
                logical_start_col: if logical_start_col == usize::MAX {
                    0
                } else {
                    logical_start_col
                },
                logical_end_col,
                visual_start_col,
                visual_end_col,
            });
        }
    }

    projection
}

#[derive(Clone, Copy, Debug)]
struct LogicalGrapheme {
    logical_start_col: usize,
    width: usize,
    whitespace: bool,
}

#[derive(Debug)]
struct WrappedRow {
    items: Vec<LogicalGrapheme>,
    width: usize,
}

fn line_graphemes(line: &Line<'_>) -> Vec<LogicalGrapheme> {
    let mut logical_col = 0usize;
    let mut graphemes = Vec::new();
    for grapheme in line.styled_graphemes(Style::default()) {
        let width = grapheme.symbol.cell_width() as usize;
        let logical_start_col = logical_col;
        logical_col = logical_col.saturating_add(width);
        graphemes.push(LogicalGrapheme {
            logical_start_col,
            width,
            whitespace: grapheme.is_whitespace(),
        });
    }
    graphemes
}

/// Mirrors `ratatui_widgets::reflow::WordWrapper::process_input` with trim=false.
fn wrap_line(line: &Line<'_>, width: usize) -> Vec<WrappedRow> {
    let mut wrapped_lines = Vec::new();
    let mut pending_line = Vec::new();
    let mut pending_word = Vec::new();
    let mut pending_whitespace: VecDeque<LogicalGrapheme> = VecDeque::new();
    let mut line_width = 0usize;
    let mut word_width = 0usize;
    let mut whitespace_width = 0usize;
    let mut non_whitespace_previous = false;

    for grapheme in line_graphemes(line) {
        // Ratatui ignores symbols wider than the paragraph width before wrapping them.
        if grapheme.width > width {
            continue;
        }

        let word_found = non_whitespace_previous && grapheme.whitespace;
        let untrimmed_overflow =
            pending_line.is_empty() && word_width + whitespace_width + grapheme.width > width;

        if word_found || untrimmed_overflow {
            pending_line.extend(pending_whitespace.drain(..));
            line_width += whitespace_width;
            pending_whitespace.clear();
            pending_line.append(&mut pending_word);
            line_width += word_width;
            whitespace_width = 0;
            word_width = 0;
        }

        let line_full = line_width >= width;
        let pending_word_overflow =
            grapheme.width > 0 && line_width + whitespace_width + word_width >= width;

        if line_full || pending_word_overflow {
            let mut remaining_width = width.saturating_sub(line_width);
            wrapped_lines.push(WrappedRow {
                items: std::mem::take(&mut pending_line),
                width: line_width,
            });
            line_width = 0;

            while let Some(next) = pending_whitespace.front() {
                if next.width > remaining_width {
                    break;
                }
                whitespace_width -= next.width;
                remaining_width -= next.width;
                pending_whitespace.pop_front();
            }

            if grapheme.whitespace && pending_whitespace.is_empty() {
                continue;
            }
        }

        if grapheme.whitespace {
            whitespace_width += grapheme.width;
            pending_whitespace.push_back(grapheme);
        } else {
            word_width += grapheme.width;
            pending_word.push(grapheme);
        }

        non_whitespace_previous = !grapheme.whitespace;
    }

    pending_line.extend(pending_whitespace.drain(..));
    line_width += whitespace_width;
    pending_line.append(&mut pending_word);
    line_width += word_width;
    if !pending_line.is_empty() {
        wrapped_lines.push(WrappedRow {
            items: pending_line,
            width: line_width,
        });
    }
    if wrapped_lines.is_empty() {
        wrapped_lines.push(WrappedRow {
            items: Vec::new(),
            width: 0,
        });
    }
    wrapped_lines
}

fn line_offset(alignment: Alignment, width: usize, line_width: usize) -> usize {
    match alignment {
        Alignment::Center => (width / 2).saturating_sub(line_width / 2),
        Alignment::Right => width.saturating_sub(line_width),
        Alignment::Left => 0,
    }
}

fn append_segment(segments: &mut Vec<VisualSegment>, next: VisualSegment) {
    if let Some(previous) = segments.last_mut() {
        if previous.logical_line == next.logical_line
            && previous.visual_row == next.visual_row
            && previous.logical_end_col == next.logical_start_col
            && previous.visual_end_col == next.visual_start_col
        {
            previous.logical_end_col = next.logical_end_col;
            previous.visual_end_col = next.visual_end_col;
            return;
        }
    }
    segments.push(next);
}
