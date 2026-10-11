use super::App;

impl App {
    pub(crate) fn diff_current_file_from_scroll(&self) -> Option<usize> {
        let state = self.diff_state.as_ref()?;
        if state.files.is_empty() {
            return None;
        }

        if let Some(pinned) = state.nav_pinned_file_idx {
            return Some(pinned.min(state.files.len() - 1));
        }

        Some(state.current_file_idx.min(state.files.len() - 1))
    }

    pub(crate) fn diff_nav_to_file(&mut self, idx: usize) -> bool {
        let Some(state) = self.diff_state.as_mut() else {
            return false;
        };
        if idx >= state.files.len() {
            return false;
        }
        let target_line = state.first_scroll_target_of_file(idx);
        state.current_file_idx = idx;
        state.nav_pinned_file_idx = Some(idx);
        state.tree_active_idx = idx;
        state.tree_scroll_manual = false;
        if let Some(line) = target_line {
            self.scroll = line;
        }
        self.clamp_scroll();
        true
    }

    pub(crate) fn diff_next_file(&mut self) -> bool {
        let Some(cur) = self.diff_current_file_from_scroll() else {
            return false;
        };
        self.diff_nav_to_file(cur + 1)
    }

    pub(crate) fn diff_prev_file(&mut self) -> bool {
        let Some(cur) = self.diff_current_file_from_scroll() else {
            return false;
        };
        if cur == 0 {
            return false;
        }
        self.diff_nav_to_file(cur - 1)
    }

    pub(crate) fn clamp_scroll(&mut self) {
        let max = self.max_scroll();
        if self.scroll > max {
            self.scroll = max;
        }
    }
}
