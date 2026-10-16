use crate::{app::App, diff::DiffFile, theme::app_theme};
use ratatui::{
    layout::{Constraint, Direction, Layout, Rect},
    style::{Modifier, Style},
    symbols::border,
    text::{Line, Span},
    widgets::{Block, Borders, Paragraph},
    Frame,
};

const DIFF_TREE_HEADER_BORDER: border::Set = border::Set {
    bottom_right: "┤",
    ..border::PLAIN
};

pub(super) fn render_diff_tree_panel(f: &mut Frame, app: &mut App, area: Rect) {
    let theme = app_theme();

    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Length(3), Constraint::Min(0)])
        .split(area);
    let header_area = chunks[0];
    let list_area = chunks[1];
    app.diff_tree_list_area = Some(list_area);

    app.ensure_diff_tree_cursor_visible();

    let is_border_hovered = app.diff_tree_dragging || app.diff_tree_hover_row.is_some();
    let border_color = if is_border_hovered {
        theme.ui.toc_accent
    } else {
        theme.ui.toc_border
    };

    f.render_widget(
        Paragraph::new("")
            .style(Style::default().bg(theme.ui.toc_bg))
            .block(
                Block::default()
                    .borders(Borders::RIGHT | Borders::BOTTOM)
                    .border_set(DIFF_TREE_HEADER_BORDER)
                    .border_style(Style::default().fg(border_color))
                    .style(Style::default().bg(theme.ui.toc_bg)),
            ),
        header_area,
    );

    let header_label = Line::from(vec![Span::styled(
        " FILE LIST",
        Style::default()
            .fg(theme.ui.toc_header_fg)
            .bg(theme.ui.toc_bg)
            .add_modifier(Modifier::BOLD),
    )]);
    f.render_widget(
        Paragraph::new(vec![header_label]).style(Style::default().bg(theme.ui.toc_bg)),
        Rect {
            x: header_area.x.saturating_add(1),
            y: header_area.y.saturating_add(1),
            width: header_area.width.saturating_sub(2),
            height: 1,
        },
    );

    let active_from_scroll = app.diff_current_file_from_scroll();

    let (rows, active_idx, total_rows) = {
        let Some(state) = app.diff() else {
            return;
        };
        let first = state.tree_scroll.min(state.files.len());
        let rows: Vec<(usize, String, (char, ratatui::style::Color))> = state
            .files
            .iter()
            .enumerate()
            .skip(first)
            .take(list_area.height as usize)
            .map(|(i, f)| (i, display_path(f), change_tag(&f.change, &theme)))
            .collect();
        let active = active_from_scroll.unwrap_or(state.tree_active_idx);
        (rows, active, state.files.len())
    };

    let mut lines: Vec<Line<'static>> = Vec::with_capacity(rows.len().max(1));
    if total_rows == 0 {
        lines.push(Line::from(Span::styled(
            "  (no files)".to_string(),
            Style::default()
                .fg(theme.ui.toc_primary_inactive)
                .bg(theme.ui.toc_bg),
        )));
    } else {
        let inner_width = list_area.width.saturating_sub(1) as usize;
        for (idx, path, tag) in &rows {
            let is_active = *idx == active_idx;
            let is_hovered = app.hovered_diff_tree_idx == Some(*idx) && !is_active;
            lines.push(build_tree_line(
                path,
                is_active,
                is_hovered,
                *tag,
                inner_width,
                &theme,
            ));
        }
    }

    let padding_width = list_area.width.saturating_sub(1) as usize;
    lines.push(Line::from(Span::styled(
        " ".repeat(padding_width),
        Style::default().bg(theme.ui.toc_bg),
    )));

    f.render_widget(
        Paragraph::new(lines)
            .style(Style::default().bg(theme.ui.toc_bg))
            .block(
                Block::default()
                    .borders(Borders::RIGHT)
                    .border_style(Style::default().fg(border_color))
                    .style(Style::default().bg(theme.ui.toc_bg)),
            ),
        list_area,
    );

    if let Some(row) = app.diff_tree_hover_row {
        if row >= header_area.y && row < list_area.y + list_area.height {
            f.render_widget(
                Paragraph::new("∙").style(Style::default().fg(border_color)),
                Rect {
                    x: list_area
                        .x
                        .saturating_add(list_area.width)
                        .saturating_sub(1),
                    y: row,
                    width: 1,
                    height: 1,
                },
            );
        }
    }
}

fn build_tree_line(
    path: &str,
    active: bool,
    hovered: bool,
    tag: (char, ratatui::style::Color),
    inner_width: usize,
    theme: &crate::theme::AppTheme,
) -> Line<'static> {
    let active_bg = theme.ui.toc_active_bg;
    let inactive_bg = theme.ui.toc_inactive_bg;
    let row_bg = if active { active_bg } else { inactive_bg };

    let marker_cols = 1;
    let spacer_cols = 1;
    let tag_cols = 1;
    let gap_cols = 1;
    let right_margin = 1;
    let reserved = marker_cols + spacer_cols + tag_cols + gap_cols + right_margin;
    let path_room = inner_width.saturating_sub(reserved).max(4);

    let display_path = truncate_head(path, path_room);

    let (dir_part, filename_part) = match display_path.rfind('/') {
        Some(idx) => (
            display_path[..=idx].to_string(),
            display_path[idx + 1..].to_string(),
        ),
        None => (String::new(), display_path.clone()),
    };

    let hover_fg = theme.ui.toc_hover_fg;
    let dir_style = Style::default()
        .fg(if hovered {
            hover_fg
        } else {
            theme.ui.toc_border
        })
        .bg(row_bg);
    let filename_style = Style::default()
        .fg(if active {
            theme.ui.toc_primary_active
        } else if hovered {
            hover_fg
        } else {
            theme.ui.toc_primary_inactive
        })
        .bg(row_bg)
        .add_modifier(Modifier::BOLD);
    let marker_style = Style::default()
        .fg(if hovered {
            hover_fg
        } else {
            theme.ui.toc_accent
        })
        .bg(row_bg);
    let (tag_char, tag_color) = tag;
    let tag_style = Style::default()
        .fg(if hovered { hover_fg } else { tag_color })
        .bg(row_bg)
        .add_modifier(Modifier::BOLD);
    let spacer_style = Style::default().bg(row_bg);

    let used = reserved + crate::markdown::display_width(&display_path);
    let pad_cols = inner_width.saturating_sub(used);
    let pad = " ".repeat(pad_cols);

    let mut spans = Vec::with_capacity(8);
    spans.push(Span::styled(if active { "▎" } else { " " }, marker_style));
    spans.push(Span::styled(" ", spacer_style));
    spans.push(Span::styled(tag_char.to_string(), tag_style));
    spans.push(Span::styled(" ", spacer_style));
    if !dir_part.is_empty() {
        spans.push(Span::styled(dir_part, dir_style));
    }
    spans.push(Span::styled(filename_part, filename_style));
    spans.push(Span::styled(pad, spacer_style));
    Line::from(spans)
}

fn change_tag(
    change: &crate::diff::FileChange,
    theme: &crate::theme::AppTheme,
) -> (char, ratatui::style::Color) {
    use crate::diff::FileChange;
    let diff = &theme.diff;
    match change {
        FileChange::Modified => ('M', diff.tree_tag_change),
        FileChange::Added => ('A', diff.add_fg),
        FileChange::Deleted => ('D', diff.del_fg),
        FileChange::Renamed { .. } => ('R', diff.tree_tag_renamed),
        FileChange::ModeOnly { .. } => ('T', diff.tree_tag_change),
        FileChange::Binary => ('B', theme.ui.toc_primary_inactive),
        FileChange::Submodule => ('S', theme.ui.toc_primary_inactive),
    }
}

fn display_path(file: &DiffFile) -> String {
    if !file.new_path.is_empty() && file.new_path != "/dev/null" {
        file.new_path.clone()
    } else {
        file.old_path.clone()
    }
}

fn truncate_head(path: &str, max: usize) -> String {
    use unicode_segmentation::UnicodeSegmentation;
    if crate::markdown::display_width(path) <= max {
        return path.to_string();
    }
    if max <= 1 {
        return "…".to_string();
    }
    let mut tail_start = path.len();
    let mut width = 0;
    for (idx, grapheme) in path.grapheme_indices(true).rev() {
        let w = crate::markdown::width::rendered_cluster_width(grapheme);
        if width + w > max - 1 {
            break;
        }
        width += w;
        tail_start = idx;
    }
    format!("…{}", &path[tail_start..])
}

#[cfg(test)]
mod tests {
    use super::truncate_head;

    #[test]
    fn truncate_head_keeps_whole_graphemes_within_the_width() {
        let path = format!("dir/{}.txt", "❤️".repeat(8));
        let cut = truncate_head(&path, 11);
        assert!(crate::markdown::display_width(&cut) <= 11, "{cut:?}");
        let combining = truncate_head("dire\u{301}e\u{301}.txt", 7);
        assert!(!combining.starts_with("…\u{301}"), "{combining:?}");
    }
}
