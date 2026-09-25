use super::{App, LinkFlash};
use crate::markdown::{LinkId, LinkOccurrence, LinkSpan};
use std::collections::HashMap;

pub(super) fn link_spans_to_map(link_spans: Vec<LinkSpan>) -> HashMap<usize, Vec<LinkSpan>> {
    let mut map: HashMap<usize, Vec<LinkSpan>> = HashMap::new();
    for span in link_spans {
        map.entry(span.line_idx).or_default().push(span);
    }
    map
}

impl App {
    pub(crate) fn set_links(
        &mut self,
        occurrences: Vec<LinkOccurrence>,
        link_spans: Vec<LinkSpan>,
    ) {
        let mut positions = link_spans
            .iter()
            .map(|span| (span.line_idx, span.start_col, span.occurrence_id))
            .collect::<Vec<_>>();
        positions.sort_unstable();
        let mut seen = std::collections::HashSet::new();
        self.link_order = positions
            .into_iter()
            .filter_map(|(_, _, id)| seen.insert(id).then_some(id))
            .collect();
        self.selected_link = None;
        self.link_occurrences = occurrences;
        self.link_spans_by_line = link_spans_to_map(link_spans);
    }

    pub(crate) fn link_destination(&self, id: LinkId) -> Option<&str> {
        self.link_occurrences
            .get(id.0)
            .filter(|occurrence| occurrence.id == id)
            .map(|occurrence| occurrence.destination.as_str())
    }
    pub(crate) fn link_at_position(
        &self,
        col: u16,
        row: u16,
        padding: u16,
        sb_width: u16,
        gutter_width: u16,
    ) -> Option<&LinkSpan> {
        self.find_hovered_link(col, row, padding, sb_width, gutter_width)
            .and_then(|(line_idx, span_idx)| {
                self.link_spans_by_line
                    .get(&line_idx)
                    .and_then(|spans| spans.get(span_idx))
            })
    }

    pub(crate) fn find_hovered_link(
        &self,
        col: u16,
        row: u16,
        padding: u16,
        sb_width: u16,
        gutter_width: u16,
    ) -> Option<(usize, usize)> {
        let area = self.content_area;
        let inner_x = area.x.saturating_add(padding);
        let inner_w = area
            .width
            .saturating_sub(padding * 2)
            .saturating_sub(sb_width);
        if col < inner_x
            || col >= inner_x.saturating_add(inner_w)
            || row < area.y
            || row >= area.y.saturating_add(area.height)
        {
            return None;
        }
        let projection = self.link_viewport_projection();
        let (line, column) =
            projection.logical_at_visual((row - area.y) as usize, (col - inner_x) as usize)?;
        let column = column.checked_sub(gutter_width as usize)?;
        let index = self
            .link_spans_by_line
            .get(&line)?
            .iter()
            .position(|span| column >= span.start_col && column < span.end_col)?;
        Some((line, index))
    }
}

impl App {
    pub(crate) fn is_link_mode(&self) -> bool {
        self.selected_link.is_some()
    }

    pub(crate) fn exit_link_mode(&mut self) {
        self.selected_link = None;
        self.clear_link_flash();
    }

    pub(crate) fn selected_link_destination(&self) -> Option<&str> {
        self.selected_link.and_then(|id| self.link_destination(id))
    }

    pub(crate) fn selected_link_index(&self) -> Option<usize> {
        let id = self.selected_link?;
        self.link_order
            .iter()
            .position(|candidate| *candidate == id)
    }

    pub(crate) fn copy_selected_link(&mut self) {
        self.copy_selected_link_with(crate::clipboard::copy_to_clipboard);
    }

    pub(crate) fn copy_selected_link_with(&mut self, copy: impl FnOnce(&str) -> bool) {
        let Some(destination) = self.selected_link_destination() else {
            return;
        };
        let copied = copy(destination);
        self.set_link_flash(if copied {
            LinkFlash::Copied
        } else {
            LinkFlash::CopyFailed
        });
    }

    pub(crate) fn open_selected_link(&mut self) {
        self.open_selected_link_with(crate::clipboard::open_url);
    }

    pub(crate) fn open_selected_link_with(&mut self, open: impl FnOnce(&str) -> bool) {
        let Some(destination) = self.selected_link_destination() else {
            return;
        };
        if !crate::clipboard::is_external_http_url(destination) {
            self.set_link_flash(LinkFlash::UnsupportedTarget);
            return;
        }
        let requested = open(destination);
        self.set_link_flash(if requested {
            LinkFlash::OpenRequested
        } else {
            LinkFlash::OpenFailed
        });
    }
}

impl App {
    fn link_content_width(&self) -> usize {
        self.content_area.width.saturating_sub(
            crate::render::CONTENT_HORIZONTAL_PADDING * 2 + crate::render::SCROLLBAR_WIDTH,
        ) as usize
    }

    fn geometry_line(&self, index: usize) -> ratatui::text::Line<'static> {
        use ratatui::text::Span;
        let mut line = self.lines[index].clone();
        if self.is_line_number_visible() {
            let digits = self.line_number_total().max(1).to_string().len();
            let logical = self.line_number_at(index);
            let first = logical > 0 && (index == 0 || self.line_number_at(index - 1) != logical);
            let number = if first {
                format!("{logical:>digits$}")
            } else {
                " ".repeat(digits)
            };
            line.spans.insert(0, Span::raw(format!("{number}│ ")));
        }
        line
    }

    pub(crate) fn link_viewport_projection(
        &self,
    ) -> crate::render::link_geometry::ViewportProjection {
        let end = self.total().min(
            self.scroll
                .saturating_add(self.content_area.height as usize),
        );
        let lines: Vec<_> = (self.scroll..end)
            .map(|index| self.geometry_line(index))
            .collect();
        let mut projection = crate::render::link_geometry::project_viewport(
            &lines,
            self.link_content_width(),
            self.content_area.height as usize,
            0,
            self.visual_scroll_offset,
        );
        for segment in &mut projection.segments {
            segment.logical_line += self.scroll;
        }
        for row in &mut projection.rows {
            row.logical_line += self.scroll;
        }
        projection
    }

    fn link_on_segment(
        &self,
        segment: &crate::render::link_geometry::VisualSegment,
    ) -> Option<LinkId> {
        let gutter = self.line_number_gutter_width();
        self.link_spans_by_line
            .get(&segment.logical_line)?
            .iter()
            .filter(|span| {
                span.start_col + gutter < segment.logical_end_col
                    && span.end_col + gutter > segment.logical_start_col
            })
            .min_by_key(|span| span.start_col)
            .map(|span| span.occurrence_id)
    }

    pub(crate) fn enter_link_mode(&mut self) {
        if self.link_order.is_empty() {
            self.set_link_flash(LinkFlash::NoLinks);
            return;
        }
        if self.link_content_width() == 0 || self.content_area.height == 0 {
            self.set_link_flash(LinkFlash::NoDisplaySpace);
            return;
        }
        let visible = self.link_viewport_projection();
        let mut selected = visible
            .segments
            .iter()
            .find_map(|segment| self.link_on_segment(segment));
        if selected.is_none() {
            for index in self.scroll..self.total() {
                if !self.link_spans_by_line.contains_key(&index) {
                    continue;
                }
                let line = self.geometry_line(index);
                let offset = if index == self.scroll {
                    self.visual_scroll_offset
                } else {
                    0
                };
                let projection = crate::render::link_geometry::project_viewport(
                    &[line],
                    self.link_content_width(),
                    usize::MAX,
                    0,
                    offset,
                );
                for mut segment in projection.segments {
                    segment.logical_line = index;
                    if let Some(id) = self.link_on_segment(&segment) {
                        selected = Some(id);
                        break;
                    }
                }
                if selected.is_some() {
                    break;
                }
            }
        }
        let Some(id) = selected else {
            self.set_link_flash(LinkFlash::NoneBelow);
            return;
        };
        self.clear_active_search();
        self.clear_active_goto_line();
        self.exit_code_select_mode();
        self.reset_numkey_state();
        self.reset_toc_scroll_mode();
        self.selected_link = Some(id);
        self.clear_link_flash();
        self.reveal_selected_link();
    }

    pub(crate) fn move_link_focus(&mut self, forward: bool) {
        let Some(index) = self.selected_link_index() else {
            return;
        };
        let count = self.link_order.len();
        let next = if forward {
            (index + 1) % count
        } else if index == 0 {
            count - 1
        } else {
            index - 1
        };
        self.selected_link = Some(self.link_order[next]);
        self.clear_link_flash();
        self.reveal_selected_link();
    }

    pub(crate) fn reveal_selected_link(&mut self) {
        let Some(id) = self.selected_link else {
            return;
        };
        let width = self.link_content_width();
        let height = self.content_area.height as usize;
        if width == 0 || height == 0 {
            return;
        }
        let gutter = self.line_number_gutter_width();
        let owns = |segment: &crate::render::link_geometry::VisualSegment, index: usize| {
            self.link_spans_by_line.get(&index).is_some_and(|spans| {
                spans.iter().any(|span| {
                    span.occurrence_id == id
                        && span.start_col + gutter < segment.logical_end_col
                        && span.end_col + gutter > segment.logical_start_col
                })
            })
        };
        if self
            .link_viewport_projection()
            .segments
            .iter()
            .any(|s| owns(s, s.logical_line))
        {
            return;
        }
        let mut indices: Vec<_> = self
            .link_spans_by_line
            .iter()
            .filter_map(|(&line, spans)| {
                spans
                    .iter()
                    .any(|span| span.occurrence_id == id)
                    .then_some(line)
            })
            .collect();
        indices.sort_unstable();
        let target = indices.into_iter().find_map(|index| {
            let line = self.geometry_line(index);
            let projection =
                crate::render::link_geometry::project_viewport(&[line], width, usize::MAX, 0, 0);
            projection
                .segments
                .iter()
                .find(|segment| owns(segment, index))
                .map(|segment| (index, segment.visual_row))
        });
        let Some((mut line, mut row)) = target else {
            return;
        };
        if (line, row) > (self.scroll, self.visual_scroll_offset) {
            let mut context = height.saturating_sub(1);
            loop {
                let consumed = row.min(context);
                row -= consumed;
                context -= consumed;
                if context == 0 || line == 0 {
                    break;
                }
                line -= 1;
                let previous = self.geometry_line(line);
                let rows = crate::render::link_geometry::project_viewport(
                    &[previous],
                    width,
                    usize::MAX,
                    0,
                    0,
                )
                .rows
                .len();
                row = rows.saturating_sub(1);
                context -= 1;
            }
        }
        self.scroll = line;
        self.visual_scroll_offset = row;
    }
}
