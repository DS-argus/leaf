pub(super) const SYNTECT_SKIP_HUNK_LINES: usize = 2000;

pub(super) struct SideHighlighters<'a> {
    syntax: Option<&'a syntect::parsing::SyntaxReference>,
    theme: &'a syntect::highlighting::Theme,
    old: Option<syntect::easy::HighlightLines<'a>>,
    new: Option<syntect::easy::HighlightLines<'a>>,
    buf: String,
}

impl<'a> SideHighlighters<'a> {
    pub(super) fn new(
        syntax: Option<&'a syntect::parsing::SyntaxReference>,
        theme: &'a syntect::highlighting::Theme,
    ) -> Self {
        let mut highlighters = Self {
            syntax,
            theme,
            old: None,
            new: None,
            buf: String::new(),
        };
        highlighters.reset();
        highlighters
    }

    pub(super) fn old_side(&mut self) -> Option<&mut syntect::easy::HighlightLines<'a>> {
        self.old.as_mut()
    }

    pub(super) fn new_side(&mut self) -> Option<&mut syntect::easy::HighlightLines<'a>> {
        self.new.as_mut()
    }

    #[allow(clippy::type_complexity)]
    pub(super) fn sides(
        &mut self,
        enabled: bool,
    ) -> (
        Option<&mut syntect::easy::HighlightLines<'a>>,
        Option<&mut syntect::easy::HighlightLines<'a>>,
    ) {
        if enabled {
            (self.old.as_mut(), self.new.as_mut())
        } else {
            (None, None)
        }
    }

    pub(super) fn advance_old(&mut self, text: &str, ss: &syntect::parsing::SyntaxSet) {
        if let Some(hl) = self.old.as_mut() {
            self.buf.clear();
            self.buf.push_str(text);
            self.buf.push('\n');
            let _ = hl.highlight_line(&self.buf, ss);
        }
    }

    pub(super) fn reset(&mut self) {
        self.old = self
            .syntax
            .map(|s| syntect::easy::HighlightLines::new(s, self.theme));
        self.new = self
            .syntax
            .map(|s| syntect::easy::HighlightLines::new(s, self.theme));
    }
}

macro_rules! assert_sidecars_parallel {
    ($lines:expr, $($sidecar:expr),+ $(,)?) => {
        $(
            debug_assert_eq!(
                $lines.len(),
                $sidecar.len(),
                concat!("sidecar ", stringify!($sidecar), " out of sync with lines"),
            );
        )+
    };
}

pub(super) use assert_sidecars_parallel;

pub(super) fn combined_row_bg(
    prefix: &str,
    diff: &crate::theme::DiffTheme,
) -> Option<ratatui::style::Color> {
    let has_plus = prefix.chars().any(|c| c == '+');
    let has_minus = prefix.chars().any(|c| c == '-');
    match (has_plus, has_minus) {
        (true, true) => Some(diff.meta_note_bg),
        (true, false) => Some(diff.add_bg),
        (false, true) => Some(diff.del_bg),
        (false, false) => None,
    }
}

fn combined_prefix_spans(
    prefix: &str,
    diff: &crate::theme::DiffTheme,
    bg: Option<ratatui::style::Color>,
) -> Vec<ratatui::text::Span<'static>> {
    use ratatui::style::Style;
    use ratatui::text::Span;
    prefix
        .chars()
        .map(|ch| {
            let fg = match ch {
                '+' => diff.add_fg,
                '-' => diff.del_fg,
                _ => diff.frame_fg,
            };
            let mut style = Style::default().fg(fg);
            if let Some(bg) = bg {
                style = style.bg(bg);
            }
            Span::styled(ch.to_string(), style)
        })
        .collect()
}

pub(super) fn build_combined_half(
    prefix: &str,
    content: &str,
    bg: Option<ratatui::style::Color>,
    diff: &crate::theme::DiffTheme,
) -> Vec<ratatui::text::Span<'static>> {
    use ratatui::style::Style;
    use ratatui::text::Span;
    let mut spans: Vec<Span<'static>> = combined_prefix_spans(prefix, diff, bg);
    let sep_style = bg.map_or(Style::default(), |b| Style::default().bg(b));
    spans.push(Span::styled(" ", sep_style));
    let mut content_style = Style::default().fg(diff.context_fg);
    if let Some(bg) = bg {
        content_style = content_style.bg(bg);
    }
    spans.push(Span::styled(content.to_string(), content_style));
    spans
}

pub(super) fn take_line_numbers(
    (in_old, in_new): (bool, bool),
    cur_old: &mut u32,
    cur_new: &mut u32,
) -> (u32, u32) {
    let o = if in_old { *cur_old } else { 0 };
    let n = if in_new { *cur_new } else { 0 };
    if in_old {
        *cur_old = cur_old.saturating_add(1);
    }
    if in_new {
        *cur_new = cur_new.saturating_add(1);
    }
    (o, n)
}
