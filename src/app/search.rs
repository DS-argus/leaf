use super::App;
use crate::markdown::hash_str;

const DIFF_MATCH_MARGIN: usize = 2;

pub(crate) struct SearchState {
    pub(super) mode: bool,
    pub(super) draft: String,
    pub(super) query: String,
    pub(super) matches: Vec<usize>,
    pub(super) idx: usize,
    pub(super) draft_hash: u64,
    pub(super) query_hash: u64,
    pub(super) layout_switch: Option<LayoutSwitch>,
}

#[derive(Clone, Copy)]
pub(crate) struct LayoutSwitch {
    from: crate::diff::DiffLayout,
    from_row: usize,
    to_row: usize,
}

impl App {
    pub(crate) fn active_highlight_line(&self) -> Option<usize> {
        if self.search.matches.is_empty() {
            None
        } else {
            Some(self.search.matches[self.search.idx])
        }
    }

    pub(crate) fn is_search_mode(&self) -> bool {
        self.search.mode
    }

    pub(crate) fn search_draft(&self) -> &str {
        &self.search.draft
    }

    pub(crate) fn search_query(&self) -> &str {
        &self.search.query
    }

    #[cfg(test)]
    pub(crate) fn set_search_query(&mut self, query: impl Into<String>) {
        self.search.query = query.into();
        self.search.query_hash = hash_str(&self.search.query);
    }

    pub(crate) fn search_match_count(&self) -> usize {
        self.search.matches.len()
    }

    pub(crate) fn search_index(&self) -> usize {
        self.search.idx
    }

    #[cfg(test)]
    pub(crate) fn search_matches(&self) -> &[usize] {
        &self.search.matches
    }

    #[cfg(test)]
    pub(crate) fn set_search_draft(&mut self, draft: impl Into<String>) {
        self.search.draft = draft.into();
        self.search.draft_hash = hash_str(&self.search.draft);
    }

    pub(crate) fn pop_search_draft(&mut self) {
        self.search.draft.pop();
        self.search.draft_hash = hash_str(&self.search.draft);
    }

    pub(crate) fn push_search_draft(&mut self, ch: char) {
        self.search.draft.push(ch);
        self.search.draft_hash = hash_str(&self.search.draft);
    }

    pub(crate) fn run_search(&mut self) {
        let q = self.search_query_lower();
        if q.is_empty() {
            return;
        }
        if self.is_diff_preview_visible() {
            self.reset_search_state();
            self.set_diff_flash(super::DiffFlash::SearchInPreview);
            return;
        }
        self.search.matches = self.search_matches_for(&q);
        self.search.idx = 0;
        self.search.layout_switch = None;
        if let Some(&f) = self.search.matches.first() {
            self.reset_toc_scroll_mode();
            self.jump_to_match(f);
        }
    }

    fn jump_to_match(&mut self, row: usize) {
        if self.is_diff_mode() {
            self.scroll = row.saturating_sub(DIFF_MATCH_MARGIN).min(self.max_scroll());
            self.pin_diff_file_of_row(row);
        } else {
            self.scroll = row.min(self.max_scroll());
        }
    }

    fn search_query_lower(&self) -> String {
        self.search.query.to_lowercase()
    }

    fn search_matches_for(&self, query_lower: &str) -> Vec<usize> {
        let source: &[String] = match self.diff() {
            Some(state) => match state.layout {
                crate::diff::DiffLayout::Unified => &state.search_lines,
                crate::diff::DiffLayout::Split => &state.split_search_lines,
            },
            None => &self.plain_lines,
        };
        source
            .iter()
            .enumerate()
            .filter(|(_, line)| line.contains(query_lower))
            .map(|(i, _)| i)
            .collect()
    }

    pub(crate) fn refresh_diff_search(&mut self) {
        let q = self.search_query_lower();
        if q.is_empty() || !self.is_diff_mode() {
            return;
        }
        self.search.matches = self.search_matches_for(&q);
        let first_visible = self.scroll + DIFF_MATCH_MARGIN;
        self.search.idx = self
            .search
            .matches
            .iter()
            .position(|&m| m >= first_visible)
            .unwrap_or(0);
    }

    pub(crate) fn remap_diff_search_after_layout(
        &mut self,
        from: crate::diff::DiffLayout,
        to: crate::diff::DiffLayout,
    ) {
        let active_row = self.search.matches.get(self.search.idx).copied();
        let q = self.search_query_lower();
        let Some(row) = active_row.filter(|_| !q.is_empty()) else {
            self.refresh_diff_search();
            return;
        };
        self.search.matches = self.search_matches_for(&q);
        let switch_back = self
            .search
            .layout_switch
            .filter(|s| s.from == to && s.to_row == row)
            .map(|s| s.from_row)
            .filter(|r| self.search.matches.binary_search(r).is_ok());
        let target = switch_back.or_else(|| {
            self.diff()
                .and_then(|state| state.convert_match_row(from, to, row, &self.search.matches))
        });
        let Some(target) = target else {
            self.search.idx = 0;
            self.search.layout_switch = None;
            return;
        };
        self.search.idx = self
            .search
            .matches
            .binary_search(&target)
            .unwrap_or_default();
        self.search.layout_switch = Some(LayoutSwitch {
            from,
            from_row: row,
            to_row: target,
        });
        self.jump_to_match(target);
    }

    pub(crate) fn begin_search(&mut self) {
        self.reset_numkey_state();
        self.search.mode = true;
        self.search.draft = self.search.query.clone();
        self.search.draft_hash = self.search.query_hash;
        crate::runtime::debug_log(
            self.debug_input,
            &format!(
                "begin_search query={:?} draft={:?} matches={} idx={}",
                self.search.query,
                self.search.draft,
                self.search.matches.len(),
                self.search.idx
            ),
        );
    }

    pub(crate) fn reset_search_state(&mut self) {
        self.search.draft.clear();
        self.search.query.clear();
        self.search.matches.clear();
        self.search.idx = 0;
        self.search.draft_hash = 0;
        self.search.query_hash = 0;
        self.search.layout_switch = None;
    }

    pub(crate) fn cancel_search(&mut self) {
        self.search.mode = false;
        self.reset_search_state();
        crate::runtime::debug_log(self.debug_input, "cancel_search cleared query and matches");
    }

    pub(crate) fn confirm_search(&mut self) {
        self.search.mode = false;
        let draft = std::mem::take(&mut self.search.draft);
        self.search.query = draft;
        self.search.query_hash = self.search.draft_hash;
        self.search.draft_hash = 0;
        if self.search.query.is_empty() {
            self.reset_search_state();
            crate::runtime::debug_log(
                self.debug_input,
                "confirm_search empty query -> cleared matches",
            );
            return;
        }
        self.run_search();
        crate::runtime::debug_log(
            self.debug_input,
            &format!(
                "confirm_search query={:?} matches={} idx={} scroll={}",
                self.search.query,
                self.search.matches.len(),
                self.search.idx,
                self.scroll
            ),
        );
    }

    pub(crate) fn clear_active_search(&mut self) {
        self.search.mode = false;
        self.reset_search_state();
        crate::runtime::debug_log(
            self.debug_input,
            "clear_active_search cleared query and matches",
        );
    }

    pub(crate) fn has_active_search(&self) -> bool {
        !self.search.query.is_empty() || !self.search.matches.is_empty()
    }

    pub(crate) fn next_match(&mut self) {
        if self.search.matches.is_empty() {
            return;
        }
        self.reset_numkey_state();
        self.reset_toc_scroll_mode();
        self.search.layout_switch = None;
        self.search.idx = (self.search.idx + 1) % self.search.matches.len();
        self.jump_to_match(self.search.matches[self.search.idx]);
    }

    pub(crate) fn prev_match(&mut self) {
        if self.search.matches.is_empty() {
            return;
        }
        self.reset_numkey_state();
        self.reset_toc_scroll_mode();
        self.search.layout_switch = None;
        if self.search.idx == 0 {
            self.search.idx = self.search.matches.len() - 1;
        } else {
            self.search.idx -= 1;
        }
        self.jump_to_match(self.search.matches[self.search.idx]);
    }
}
