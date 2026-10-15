use super::App;
use crate::markdown::{LinkId, LinkSpan};
use std::collections::HashMap;

pub(super) fn link_spans_to_map(link_spans: Vec<LinkSpan>) -> HashMap<usize, Vec<LinkSpan>> {
    let mut map: HashMap<usize, Vec<LinkSpan>> = HashMap::new();
    for span in link_spans {
        map.entry(span.line_idx).or_default().push(span);
    }
    map
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct ScreenGeometryCacheKey {
    scroll: usize,
    width: u16,
    height: u16,
    gutter: usize,
}

impl App {
    pub(crate) fn set_links(&mut self, link_urls: Vec<String>, link_spans: Vec<LinkSpan>) {
        self.link_urls = link_urls;
        self.link_spans_by_line = link_spans_to_map(link_spans);
        self.screen_geometry = crate::render::ScreenGeometry::default();
        self.screen_geometry_cache = None;
    }

    pub(crate) fn link_url(&self, id: LinkId) -> Option<&str> {
        self.link_urls.get(id.0).map(String::as_str)
    }

    pub(crate) fn link_at_position(&self, col: u16, row: u16) -> Option<&LinkSpan> {
        let (line, index) = self.find_hovered_link(col, row)?;
        self.link_spans_by_line.get(&line)?.get(index)
    }

    pub(crate) fn find_hovered_link(&self, col: u16, row: u16) -> Option<(usize, usize)> {
        let (column, row) = self.screen_cell(col, row)?;
        let links = &self.screen_geometry.links;
        let first = links.partition_point(|link| link.row < row);
        links[first..]
            .iter()
            .take_while(|link| link.row == row)
            .find(|link| link.start_col <= column && column < link.end_col)
            .map(|link| (self.screen_geometry.row_lines[row], link.span_idx))
    }

    pub(crate) fn line_idx_at(&self, col: u16, row: u16) -> Option<usize> {
        let (_, row) = self.screen_cell(col, row)?;
        self.screen_geometry.row_lines.get(row).copied()
    }

    fn screen_cell(&self, col: u16, row: u16) -> Option<(usize, usize)> {
        let area = crate::render::inner_content_area(self.content_area);
        if !area.contains(ratatui::layout::Position::new(col, row)) {
            return None;
        }
        Some((usize::from(col - area.x), usize::from(row - area.y)))
    }

    pub(crate) fn refresh_screen_geometry(
        &mut self,
        lines: &[ratatui::text::Line<'_>],
        width: u16,
        height: u16,
    ) {
        let key = ScreenGeometryCacheKey {
            scroll: self.scroll,
            width,
            height,
            gutter: self.line_number_gutter_width(),
        };
        if self.screen_geometry_cache == Some(key) {
            return;
        }
        self.screen_geometry = crate::render::screen_geometry(
            lines,
            &self.link_spans_by_line,
            key.scroll,
            key.gutter,
            width,
            height,
        );
        self.screen_geometry_cache = Some(key);
    }
}
