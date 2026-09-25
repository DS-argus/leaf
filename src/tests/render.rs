use super::{find_symbol, render_buffer, test_assets, test_md_theme};
use crate::app::App;
use crate::markdown::{parse_markdown, parse_markdown_with_width};
use crate::render::link_geometry::project_viewport;
use crate::wrap_path_lines;
use ratatui::{
    backend::TestBackend,
    style::Style,
    text::Line,
    widgets::{Paragraph, Wrap},
    Terminal,
};

fn wrapped_rows_at(
    lines: &[Line<'static>],
    width: u16,
    height: u16,
    visual_scroll: usize,
) -> Vec<String> {
    let mut terminal = Terminal::new(TestBackend::new(width, height)).unwrap();
    terminal
        .draw(|f| {
            f.render_widget(
                Paragraph::new(lines.to_vec())
                    .wrap(Wrap { trim: false })
                    .scroll((visual_scroll.min(u16::MAX as usize) as u16, 0)),
                f.area(),
            );
        })
        .unwrap();
    let buffer = terminal.backend().buffer().clone();
    (0..height)
        .map(|y| {
            (0..width)
                .map(|x| buffer.cell((x, y)).unwrap().symbol())
                .collect()
        })
        .collect()
}

fn wrapped_rows(lines: &[Line<'static>], width: u16, height: u16) -> Vec<String> {
    wrapped_rows_at(lines, width, height, 0)
}

#[test]
fn code_block_box_renders_right_border_in_one_column() {
    let (ss, theme) = test_assets();
    let md = "```ts\nconst city = \"東京\";\n\tconsole.log(city)\n```";
    let (lines, _, _, _) = parse_markdown(md, &ss, &theme, &test_md_theme(), false, true).into();
    let buffer = render_buffer(&lines);

    let (right_x, start_y) = find_symbol(&buffer, "┐").unwrap();
    let (_, end_y) = find_symbol(&buffer, "┘").unwrap();

    for y in start_y + 1..end_y {
        assert_eq!(
            buffer.cell((right_x, y)).unwrap().symbol(),
            "│",
            "missing code block right border at row {y}"
        );
    }
}

#[test]
fn file_mode_code_block_fills_full_render_width() {
    let (ss, theme) = test_assets();
    let render_width = 40;
    let src = App::fence_wrap("fn main() {\n    let city = \"東京\";\n}", "rs");
    let (lines, _, _, _) = parse_markdown_with_width(
        &src,
        &ss,
        &theme,
        render_width,
        &test_md_theme(),
        true,
        true,
    )
    .into();
    let buffer = render_buffer(&lines);

    assert!(find_symbol(&buffer, "┐").is_some());
    assert!(find_symbol(&buffer, "┘").is_some());
    for line in lines.iter().filter(|line| line.width() > 0) {
        assert_eq!(
            line.width(),
            render_width,
            "code block line should fill the render width"
        );
    }
}

#[test]
fn table_render_right_border_stays_aligned() {
    let (ss, theme) = test_assets();
    let md = "| Name | Value |\n| --- | --- |\n| 東京 | 12 |\n| tab\tcell | ok |";
    let (lines, _, _, _) = parse_markdown(md, &ss, &theme, &test_md_theme(), false, true).into();
    let buffer = render_buffer(&lines);

    let (right_x, start_y) = find_symbol(&buffer, "┐").unwrap();
    let (_, end_y) = find_symbol(&buffer, "┘").unwrap();

    for y in start_y + 1..end_y {
        let symbol = buffer.cell((right_x, y)).unwrap().symbol();
        assert!(
            matches!(symbol, "│" | "┤" | "╡"),
            "unexpected table edge symbol {symbol:?} at row {y}"
        );
    }
}

#[test]
fn table_render_right_border_stays_aligned_with_emoji_cells() {
    let (ss, theme) = test_assets();
    let md = "| Critère | Note |\n| --- | --- |\n| Tests | ✅ Bonne couverture |\n| Sécurité | ⚠ Quelques points |\n";
    let (lines, _, _, _) = parse_markdown(md, &ss, &theme, &test_md_theme(), false, true).into();
    let buffer = render_buffer(&lines);

    let (right_x, start_y) = find_symbol(&buffer, "┐").unwrap();
    let (_, end_y) = find_symbol(&buffer, "┘").unwrap();

    for y in start_y + 1..end_y {
        let symbol = buffer.cell((right_x, y)).unwrap().symbol();
        assert!(
            matches!(symbol, "│" | "┤" | "╡"),
            "unexpected emoji-table edge symbol {symbol:?} at row {y}"
        );
    }
}

fn plain(lines: &[ratatui::text::Line<'_>]) -> Vec<String> {
    lines
        .iter()
        .map(|l| {
            l.spans
                .iter()
                .map(|s| s.content.as_ref())
                .collect::<String>()
        })
        .collect()
}

#[test]
fn wrap_path_lines_short_path_fits_single_line() {
    let s = Style::default();
    let lines = wrap_path_lines("Relative: ", "src/main.rs", 74, s, s);
    assert_eq!(lines.len(), 1);
    assert_eq!(plain(&lines), vec!["Relative: src/main.rs"]);
}

#[test]
fn wrap_path_lines_long_path_wraps_with_indent() {
    let s = Style::default();
    let label = "Absolute: ";
    let path = "a".repeat(80);
    let lines = wrap_path_lines(label, &path, 30, s, s);
    let text = plain(&lines);
    assert!(lines.len() > 1);
    assert!(text[0].starts_with("Absolute: "));
    for continuation in &text[1..] {
        assert!(
            continuation.starts_with("          "),
            "continuation should be indented by label width"
        );
    }
}

#[test]
fn wrap_path_lines_continuation_aligned_with_value_start() {
    let s = Style::default();
    let label = "Relative: ";
    let path = "x".repeat(100);
    let lines = wrap_path_lines(label, &path, 40, s, s);
    let text = plain(&lines);
    let value_width = 40 - label.len();
    assert_eq!(&text[0], &format!("Relative: {}", &path[..value_width]));
    assert_eq!(
        &text[1],
        &format!(
            "{}{}",
            " ".repeat(label.len()),
            &path[value_width..value_width * 2]
        )
    );
}

#[test]
fn wrap_path_lines_exact_fit_no_wrap() {
    let s = Style::default();
    let label = "Test: ";
    let path = "x".repeat(74 - label.len());
    let lines = wrap_path_lines(label, &path, 74, s, s);
    assert_eq!(lines.len(), 1);
}

#[test]
fn wrap_path_lines_one_char_over_wraps() {
    let s = Style::default();
    let label = "Test: ";
    let path = "x".repeat(74 - label.len() + 1);
    let lines = wrap_path_lines(label, &path, 74, s, s);
    assert_eq!(lines.len(), 2);
}

#[test]
fn path_popup_copy_status_uses_reserved_row_without_moving_paths() {
    use crate::app::{PathKind, FLASH_DURATION_MS};
    use ratatui::{backend::TestBackend, buffer::Buffer, Terminal};
    use std::time::{Duration, Instant};

    fn draw(app: &mut App, width: u16) -> Buffer {
        let mut terminal = Terminal::new(TestBackend::new(width, 30)).unwrap();
        terminal.draw(|f| crate::render::ui(f, app)).unwrap();
        terminal.backend().buffer().clone()
    }

    let _guard = super::lock_theme_test_state();
    for width in [80, 60] {
        for length in [55, 56, 63, 64, 65] {
            let mut app = App::new(
                vec![],
                vec![],
                "test".to_string(),
                false,
                false,
                Some(std::path::PathBuf::from("x".repeat(length))),
                None,
            );
            app.open_path_popup();
            let idle = draw(&mut app, width);
            let relative_area = app.path_popup_rel_area.unwrap();
            let absolute_area = app.path_popup_abs_area.unwrap();
            let status_y = absolute_area.bottom();

            for (target, copied_message) in [
                (PathKind::Relative, "Relative path copied to clipboard"),
                (PathKind::Absolute, "Absolute path copied to clipboard"),
            ] {
                for success in [true, false] {
                    app.path_copy_flash = Some((target.clone(), success, Instant::now()));
                    let active = draw(&mut app, width);
                    let expected = if success {
                        copied_message.to_string()
                    } else {
                        match std::env::consts::OS {
                            "macos" => "Copy failed: pbcopy not found",
                            "windows" => "Copy failed: clip.exe not found",
                            _ => "Copy failed: install xclip or wl-clipboard",
                        }
                        .to_string()
                    };
                    let status: String = (absolute_area.x..absolute_area.right())
                        .map(|x| active.cell((x, status_y)).unwrap().symbol())
                        .collect();
                    assert_eq!(
                        status.trim_end(),
                        expected,
                        "width={width}, length={length}"
                    );
                    assert_eq!(app.path_popup_rel_area, Some(relative_area));
                    assert_eq!(app.path_popup_abs_area, Some(absolute_area));
                    for y in 0..active.area.height {
                        if y != status_y {
                            for x in 0..active.area.width {
                                assert_eq!(active.cell((x, y)), idle.cell((x, y)));
                            }
                        }
                    }
                }
            }

            app.path_copy_flash = Some((
                PathKind::Absolute,
                true,
                Instant::now() - Duration::from_millis(FLASH_DURATION_MS + 1),
            ));
            assert_eq!(draw(&mut app, width), idle, "expired status must clear");
        }
    }
}

#[test]
fn heading_link_is_far_below_parsed_line_in_wrapped_viewport() {
    let (ss, theme) = test_assets();
    let words = std::iter::repeat_n("abcdefghij", 40)
        .collect::<Vec<_>>()
        .join(" ");
    let source = format!("# {words} [late](https://example.test/late)\n");
    let parsed = parse_markdown_with_width(&source, &ss, &theme, 20, &test_md_theme(), false, true);
    let lines = &parsed.lines;
    let late = parsed
        .link_spans
        .iter()
        .find(|span| {
            parsed.link_occurrences[span.occurrence_id.0].destination == "https://example.test/late"
        })
        .expect("heading link should have a parsed span");
    assert_eq!(late.line_idx, 0, "the heading remains one parsed line");
    assert!(
        late.start_col > 20,
        "the target is beyond parsed viewport width"
    );
    assert!(lines[late.line_idx].width() > 20);

    let viewport = wrapped_rows(lines, 20, 3);
    assert!(viewport.iter().all(|row| !row.contains("late")));

    let rows = wrapped_rows(lines, 20, 45);
    let late_row = rows.iter().position(|row| row.contains("late"));
    assert_eq!(late_row, Some(39), "Paragraph's trim:false row projection");

    let projection = project_viewport(lines, 20, 45, 0, 0);
    let mapped = projection
        .visual_for_logical(0, late.start_col)
        .expect("target should be projected in the full viewport");
    assert_eq!(mapped.visual_row, late_row.unwrap());
    let target_column = mapped.visual_start_col + late.start_col - mapped.logical_start_col;
    assert_eq!(
        projection.logical_at_visual(mapped.visual_row, target_column),
        Some((0, late.start_col))
    );
    assert_eq!(
        projection.logical_at_visual(mapped.visual_row, target_column + 2),
        Some((0, late.start_col + 2)),
        "a later cell in one merged segment retains its logical column",
    );

    let offset_viewport = wrapped_rows_at(lines, 20, 3, 39);
    assert!(offset_viewport[0].contains("late"));

    let offset_projection = project_viewport(lines, 20, 3, 0, 39);
    let offset_segment = offset_projection
        .visual_for_logical(0, late.start_col)
        .expect("within-line offset should reach the target");
    assert_eq!(offset_segment.visual_row, 0);
    assert!(project_viewport(lines, 20, 3, 0, 0)
        .visual_for_logical(0, late.start_col)
        .is_none());
}

#[test]
fn narrow_terminal_rewraps_subtwenty_paragraph_lines() {
    let (ss, theme) = test_assets();
    let source = "alpha beta gamma delta epsilon [late](https://example.test/late)\n";
    let parsed = parse_markdown_with_width(source, &ss, &theme, 20, &test_md_theme(), false, true);
    let lines = &parsed.lines;
    let late = parsed
        .link_spans
        .iter()
        .find(|span| {
            parsed.link_occurrences[span.occurrence_id.0].destination == "https://example.test/late"
        })
        .expect("paragraph link should have a parsed span");
    assert_eq!(
        late.line_idx, 1,
        "the parser wraps this paragraph at width 20"
    );
    assert!(lines[late.line_idx].width() > 12);

    let height = (lines.len().saturating_mul(4).max(8)) as u16;
    let wide_rows = wrapped_rows(lines, 20, height);
    let narrow_rows = wrapped_rows(lines, 12, height);
    let wide_marker_row = wide_rows
        .iter()
        .position(|row| row.contains('#'))
        .expect("wide paragraph should render its link marker");
    let narrow_marker_row = narrow_rows
        .iter()
        .position(|row| row.contains('#'))
        .expect("narrow paragraph should render its link marker");

    let wide_projection = project_viewport(lines, 20, height as usize, 0, 0);
    let narrow_projection = project_viewport(lines, 12, height as usize, 0, 0);
    assert_eq!(
        wide_projection
            .visual_for_logical(late.line_idx, late.start_col)
            .map(|segment| segment.visual_row),
        Some(wide_marker_row)
    );
    assert_eq!(
        narrow_projection
            .visual_for_logical(late.line_idx, late.start_col)
            .map(|segment| segment.visual_row),
        Some(narrow_marker_row)
    );

    assert_eq!(wide_marker_row, late.line_idx);
    assert!(
        narrow_marker_row > wide_marker_row,
        "a sub-20 Paragraph viewport rewraps the parsed line"
    );
}

#[test]
fn link_projection_matches_wrapped_cells_for_unicode_alignment_and_offsets() {
    use ratatui::{buffer::CellWidth, layout::Alignment, text::Span};
    for text in [
        "  alpha beta   gamma ",
        "가나다 é 👩‍💻 xyz",
        "abcdefghijklmnopqrst",
        "  12│ hello world",
        "a\u{00a0}b c",
    ] {
        for alignment in [Alignment::Left, Alignment::Center, Alignment::Right] {
            let line = Line::from(vec![Span::raw(text)]).alignment(alignment);
            let mut source_cells = Vec::new();
            for grapheme in line.styled_graphemes(Style::default()) {
                let width = grapheme.symbol.cell_width() as usize;
                if width > 0 {
                    source_cells.push(Some(grapheme.symbol.to_string()));
                    source_cells.extend((1..width).map(|_| None));
                }
            }
            for width in 1..24u16 {
                for offset in 0..3 {
                    let mut terminal = Terminal::new(TestBackend::new(width + 3, 8)).unwrap();
                    terminal
                        .draw(|frame| {
                            frame.render_widget(
                                Paragraph::new(vec![line.clone()])
                                    .wrap(Wrap { trim: false })
                                    .scroll((offset as u16, 0)),
                                ratatui::layout::Rect::new(1, 0, width, 8),
                            )
                        })
                        .unwrap();
                    let projection =
                        project_viewport(std::slice::from_ref(&line), width as usize, 8, 0, offset);
                    for segment in &projection.segments {
                        for (column, expected) in source_cells
                            .iter()
                            .enumerate()
                            .take(segment.logical_end_col)
                            .skip(segment.logical_start_col)
                        {
                            if let Some(expected) = expected {
                                let x =
                                    segment.visual_start_col + column - segment.logical_start_col;
                                let cell = &terminal.backend().buffer()
                                    [((x + 1) as u16, segment.visual_row as u16)];
                                assert_eq!(cell.symbol(), expected, "text={text:?} width={width} offset={offset} alignment={alignment:?} x={x} y={} projection={projection:?} buffer={:?}", segment.visual_row, terminal.backend().buffer());
                                assert_eq!(
                                    projection.logical_at_visual(segment.visual_row, x),
                                    Some((0, column))
                                );
                            }
                        }
                    }
                }
            }
        }
    }
}
