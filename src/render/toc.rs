use crate::{app::App, theme::app_theme};
use ratatui::{
    layout::{Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    symbols::border,
    text::{Line, Span},
    widgets::{Block, Borders, Paragraph},
    Frame,
};

const TOC_HEADER_BORDER: border::Set = border::Set {
    bottom_right: "┤",
    ..border::PLAIN
};

pub(super) fn render_toc_panel(f: &mut Frame, app: &mut App, area: Rect) {
    let theme = app_theme();
    app.refresh_toc_cache();
    let toc_chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Length(3), Constraint::Min(0)])
        .split(area);
    app.toc_list_area = Some(toc_chunks[1]);

    let is_border_hovered = app.toc_dragging || app.toc_hover_row.is_some();
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
                    .border_set(TOC_HEADER_BORDER)
                    .border_style(Style::default().fg(border_color))
                    .style(Style::default().bg(theme.ui.toc_bg)),
            ),
        toc_chunks[0],
    );

    let scroll_offset = app.toc_scroll_offset(toc_chunks[1].height);
    let mut lines: Vec<Line<'static>> = app.toc_display_lines().to_vec();
    if let Some(display_idx) = app.hovered_toc_idx {
        let is_active = app.toc_display_entries().get(display_idx).copied() == app.toc_active_idx;
        if !is_active {
            if let Some(line) = lines.get_mut(display_idx) {
                apply_toc_hover_style(line, theme.ui.toc_hover_fg);
            }
        }
    }
    let padding_width = toc_chunks[1].width.saturating_sub(1) as usize;
    lines.push(Line::from(Span::styled(
        " ".repeat(padding_width),
        Style::default().bg(theme.ui.toc_bg),
    )));
    f.render_widget(
        Paragraph::new(lines)
            .scroll((scroll_offset as u16, 0))
            .style(Style::default().bg(theme.ui.toc_bg))
            .block(
                Block::default()
                    .borders(Borders::RIGHT)
                    .border_style(Style::default().fg(border_color))
                    .style(Style::default().bg(theme.ui.toc_bg)),
            ),
        toc_chunks[1],
    );
    f.render_widget(
        Paragraph::new(vec![app.toc_header_line().clone()])
            .style(Style::default().bg(theme.ui.toc_bg)),
        Rect {
            x: toc_chunks[0].x.saturating_add(1),
            y: toc_chunks[0].y.saturating_add(1),
            width: toc_chunks[0].width.saturating_sub(2),
            height: 1,
        },
    );

    if let Some(row) = app.toc_hover_row {
        let panel_top = toc_chunks[0].y;
        let panel_bottom = toc_chunks[1].y + toc_chunks[1].height;
        if row >= panel_top && row < panel_bottom {
            f.render_widget(
                Paragraph::new("∙").style(Style::default().fg(border_color)),
                Rect {
                    x: toc_chunks[1].x + toc_chunks[1].width - 1,
                    y: row,
                    width: 1,
                    height: 1,
                },
            );
        }
    }
}

fn apply_toc_hover_style(line: &mut Line<'static>, hover_fg: Color) {
    for span in &mut line.spans {
        span.style = span.style.fg(hover_fg);
    }
}

pub(crate) fn toc_header_line() -> Line<'static> {
    let theme = app_theme();
    Line::from(vec![Span::styled(
        " TABLE OF CONTENTS",
        Style::default()
            .fg(theme.ui.toc_header_fg)
            .bg(theme.ui.toc_bg)
            .add_modifier(Modifier::BOLD),
    )])
}

pub(crate) fn build_toc_line_with_index(
    entry: &crate::markdown::toc::TocEntry,
    display_level: u8,
    top_level_index: Option<usize>,
    active: bool,
    panel_width: u16,
) -> Line<'static> {
    let theme = app_theme();
    let active_bg = theme.ui.toc_active_bg;
    let inactive_bg = theme.ui.toc_inactive_bg;

    match display_level {
        1 => {
            let index = top_level_index.unwrap_or(0) + 1;
            let index_str = format!("{index:02}");
            let title = crate::markdown::truncate_display_width(
                &entry.title,
                (panel_width as usize).saturating_sub(7),
            );
            let bg = if active { active_bg } else { inactive_bg };

            let actual_used =
                1 + 1 + index_str.len() + 1 + crate::markdown::width::display_width(&title);
            let inner_width = (panel_width as usize).saturating_sub(1);
            let pad_cols = inner_width.saturating_sub(actual_used);
            let pad = " ".repeat(pad_cols);

            Line::from(vec![
                Span::styled(
                    if active { "▎" } else { " " },
                    Style::default().fg(theme.ui.toc_accent).bg(bg),
                ),
                Span::styled(" ", Style::default().bg(bg)),
                Span::styled(
                    index_str,
                    Style::default()
                        .fg(if active {
                            theme.ui.toc_accent
                        } else {
                            theme.ui.toc_index_inactive
                        })
                        .bg(bg)
                        .add_modifier(Modifier::BOLD),
                ),
                Span::styled(" ", Style::default().bg(bg)),
                Span::styled(
                    title,
                    Style::default()
                        .fg(if active {
                            theme.ui.toc_primary_active
                        } else {
                            theme.ui.toc_primary_inactive
                        })
                        .bg(bg)
                        .add_modifier(Modifier::BOLD),
                ),
                Span::styled(pad, Style::default().bg(bg)),
            ])
        }
        _ => {
            let title = crate::markdown::truncate_display_width(
                &entry.title,
                (panel_width as usize).saturating_sub(9),
            );
            let bg = if active { active_bg } else { inactive_bg };

            let actual_used = 1 + 4 + 1 + 1 + crate::markdown::width::display_width(&title);
            let inner_width = (panel_width as usize).saturating_sub(1);
            let pad_cols = inner_width.saturating_sub(actual_used);
            let pad = " ".repeat(pad_cols);

            Line::from(vec![
                Span::styled(
                    if active { "▎" } else { " " },
                    Style::default().fg(theme.ui.toc_accent).bg(bg),
                ),
                Span::styled("    ", Style::default().bg(bg)),
                Span::styled(
                    "•",
                    Style::default()
                        .fg(if active {
                            theme.ui.toc_accent
                        } else {
                            theme.ui.toc_secondary_inactive
                        })
                        .bg(bg),
                ),
                Span::styled(" ", Style::default().bg(bg)),
                Span::styled(
                    title,
                    Style::default()
                        .fg(if active {
                            theme.ui.toc_secondary_text_active
                        } else {
                            theme.ui.toc_secondary_text_inactive
                        })
                        .bg(bg)
                        .add_modifier(if active {
                            Modifier::BOLD
                        } else {
                            Modifier::empty()
                        }),
                ),
                Span::styled(pad, Style::default().bg(bg)),
            ])
        }
    }
}
