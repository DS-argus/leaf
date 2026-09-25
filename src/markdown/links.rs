use ratatui::text::{Line, Span};

use super::width::iter_cluster_widths;

/// Stable identity for one parser-level link occurrence.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub(crate) struct LinkId(pub(crate) usize);

/// Destination registered for one parser-level link occurrence.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct LinkOccurrence {
    pub(crate) id: LinkId,
    pub(crate) destination: String,
}

/// Per-parse link registry. The parser owns one registry for its whole run;
/// snapshots of delayed content must not copy or rewind it.
#[derive(Debug, Default)]
pub(crate) struct LinkRegistry {
    occurrences: Vec<LinkOccurrence>,
}

impl LinkRegistry {
    pub(crate) fn new() -> Self {
        Self::default()
    }

    pub(crate) fn register(&mut self, destination: &str) -> LinkId {
        let id = LinkId(self.occurrences.len());
        self.occurrences.push(LinkOccurrence {
            id,
            destination: destination.to_owned(),
        });
        id
    }

    pub(crate) fn into_occurrences(self) -> Vec<LinkOccurrence> {
        self.occurrences
    }
}

/// A styled inline fragment carrying its parser-level link owner separately
/// from presentation style.
#[derive(Clone, Debug)]
pub(super) struct LinkedSpan {
    pub(super) span: Span<'static>,
    pub(super) link_id: Option<LinkId>,
}

impl LinkedSpan {
    pub(super) fn new(span: Span<'static>, link_id: Option<LinkId>) -> Self {
        Self { span, link_id }
    }
}

/// Exact terminal-cell ownership range for one rendered line.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct LinkSpan {
    pub(crate) occurrence_id: LinkId,
    pub(crate) line_idx: usize,
    pub(crate) start_col: usize,
    pub(crate) end_col: usize,
}

/// Append one ordinary ratatui line and its exact link ranges together.
///
/// Prefixes, alignment fill, and all other structural cells are represented by
/// `LinkedSpan`s with `link_id == None`. Ranges are formed only after those
/// cells have contributed their terminal width, and touching ranges belonging
/// to the same occurrence are coalesced.
pub(super) fn emit_linked_line(
    lines: &mut Vec<Line<'static>>,
    ranges: &mut Vec<LinkSpan>,
    linked_spans: Vec<LinkedSpan>,
) {
    let line_idx = lines.len();
    let mut col = 0usize;
    let mut spans = Vec::with_capacity(linked_spans.len());

    for linked in linked_spans {
        let width = linked.span.width();
        if let Some(occurrence_id) = linked.link_id {
            if width > 0 {
                let start_col = col;
                let end_col = col + width;
                if let Some(previous) = ranges.last_mut() {
                    if previous.line_idx == line_idx
                        && previous.occurrence_id == occurrence_id
                        && previous.end_col == start_col
                    {
                        previous.end_col = end_col;
                    } else {
                        ranges.push(LinkSpan {
                            occurrence_id,
                            line_idx,
                            start_col,
                            end_col,
                        });
                    }
                } else {
                    ranges.push(LinkSpan {
                        occurrence_id,
                        line_idx,
                        start_col,
                        end_col,
                    });
                }
            }
        }
        col += width;
        spans.push(linked.span);
    }

    lines.push(Line::from(spans));
}

/// Restore explicit ownership metadata onto an ordinary line.
///
/// This is used when delayed content (for example a footnote definition) is
/// captured as ordinary lines and later rewrapped. Ownership is read only from
/// the supplied ranges; styles and marker glyphs are never inspected.
pub(super) fn restore_linked_spans(
    line_idx: usize,
    line: &Line<'_>,
    ranges: &[LinkSpan],
) -> Vec<LinkedSpan> {
    let mut restored = Vec::new();
    let mut col = 0usize;

    for span in &line.spans {
        let style = span.style;
        if span.content.is_empty() {
            restored.push(LinkedSpan::new(Span::styled("", style), None));
            continue;
        }
        let mut current_owner = None;
        let mut current_text = String::new();

        for (cluster, nominal_width) in iter_cluster_widths(span.content.as_ref()) {
            let width = nominal_width;
            let owner = if width == 0 {
                current_owner
            } else {
                owner_for_range(line_idx, col, col + width, ranges)
            };

            if current_owner == owner {
                current_text.push_str(cluster);
            } else {
                if !current_text.is_empty() {
                    restored.push(LinkedSpan::new(
                        Span::styled(std::mem::take(&mut current_text), style),
                        current_owner,
                    ));
                }
                current_owner = owner;
                current_text.push_str(cluster);
            }
            col += width;
        }

        if !current_text.is_empty() {
            restored.push(LinkedSpan::new(
                Span::styled(current_text, style),
                current_owner,
            ));
        }
    }

    restored
}

fn owner_for_range(
    line_idx: usize,
    start_col: usize,
    end_col: usize,
    ranges: &[LinkSpan],
) -> Option<LinkId> {
    ranges.iter().find_map(|range| {
        (range.line_idx == line_idx && range.start_col <= start_col && end_col <= range.end_col)
            .then_some(range.occurrence_id)
    })
}
