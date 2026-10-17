use super::{App, LinkFlash};
use crate::markdown::{LinkId, LinkSpan};
use std::collections::{HashMap, HashSet};

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
        self.link_select = None;
    }

    pub(crate) fn link_url(&self, id: LinkId) -> Option<&str> {
        self.link_urls.get(id.0).map(String::as_str)
    }

    pub(crate) fn link_span(&self, line_idx: usize, span_idx: usize) -> Option<&LinkSpan> {
        self.link_spans_by_line.get(&line_idx)?.get(span_idx)
    }

    pub(crate) fn link_at_position(&self, col: u16, row: u16) -> Option<&LinkSpan> {
        let (line, index) = self.find_hovered_link(col, row)?;
        self.link_span(line, index)
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
        self.retain_visible_link_selection();
    }

    fn visible_link_ids(&self) -> Vec<LinkId> {
        let mut seen = HashSet::new();
        self.screen_geometry
            .links
            .iter()
            .filter_map(|link| {
                self.link_span(self.screen_geometry.row_lines[link.row], link.span_idx)
            })
            .map(|span| span.link_id)
            .filter(|id| seen.insert(*id))
            .collect()
    }

    pub(crate) fn is_link_select_mode(&self) -> bool {
        self.link_select.is_some()
    }

    pub(crate) fn selected_link(&self) -> Option<LinkId> {
        self.link_select
    }

    pub(crate) fn selected_link_url(&self) -> Option<&str> {
        self.link_url(self.link_select?)
    }

    pub(crate) fn enter_link_select_mode(&mut self) {
        match self.visible_link_ids().first() {
            Some(&id) => {
                self.clear_active_goto_line();
                self.link_select = Some(id);
            }
            None => self.set_link_flash(LinkFlash::NoneVisible),
        }
    }

    pub(crate) fn exit_link_select_mode(&mut self) -> bool {
        let was_active = self.link_select.is_some();
        self.link_select = None;
        was_active
    }

    pub(crate) fn link_select_next(&mut self) {
        self.step_link_selection(true);
    }

    pub(crate) fn link_select_prev(&mut self) {
        self.step_link_selection(false);
    }

    fn step_link_selection(&mut self, forward: bool) {
        let ids = self.visible_link_ids();
        let Some(index) = ids.iter().position(|id| Some(*id) == self.link_select) else {
            self.link_select = None;
            return;
        };
        let next = if forward {
            (index + 1) % ids.len()
        } else {
            (index + ids.len() - 1) % ids.len()
        };
        self.link_select = Some(ids[next]);
    }

    fn retain_visible_link_selection(&mut self) {
        if self
            .link_select
            .is_some_and(|id| !self.visible_link_ids().contains(&id))
        {
            self.link_select = None;
        }
    }

    fn link_target(&self, id: LinkId) -> Option<LinkTarget<'_>> {
        let destination = self.link_url(id)?;
        Some(if destination.starts_with('#') {
            LinkTarget::CurrentFile
        } else {
            LinkTarget::Url(destination)
        })
    }

    pub(crate) fn copy_link_with(&mut self, id: LinkId, copy: impl FnOnce(&str) -> bool) {
        let copied = match self.link_target(id) {
            Some(LinkTarget::Url(url)) => copy(url),
            Some(LinkTarget::CurrentFile) => copy(self.filename()),
            None => return,
        };
        self.link_select = None;
        self.set_link_flash(if copied {
            LinkFlash::Copied
        } else {
            LinkFlash::CopyFailed
        });
    }

    pub(crate) fn open_link_with(&self, id: LinkId, open: impl FnOnce(&str)) {
        match self.link_target(id) {
            Some(LinkTarget::Url(url)) => open(url),
            Some(LinkTarget::CurrentFile) => {
                if let Some(path) = self.filepath() {
                    open(&path.display().to_string());
                }
            }
            None => {}
        }
    }
}

enum LinkTarget<'a> {
    Url(&'a str),
    CurrentFile,
}
