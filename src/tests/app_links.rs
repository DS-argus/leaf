use super::{lock_theme_test_state, test_assets, test_md_theme};
use crate::app::{App, AppConfig, LinkFlash};
use crate::markdown::parse_markdown_with_width;
use crate::theme::app_theme;
use ratatui::{backend::TestBackend, buffer::Buffer, layout::Rect, Terminal};

fn app_for(source: &str, width: usize, height: usize) -> App {
    let (ss, theme) = test_assets();
    let parsed =
        parse_markdown_with_width(source, &ss, &theme, width, &test_md_theme(), false, true);
    let mut app = App::new_with_source(
        Vec::new(),
        Vec::new(),
        AppConfig {
            filename: "links.md".into(),
            source: source.into(),
            debug_input: false,
            watch: false,
            filepath: None,
            last_file_state: None,
        },
    );
    app.replace_content(parsed);
    app.content_area = Rect::new(0, 0, width as u16 + 3, height as u16);
    app
}

#[test]
fn link_navigation_cycles_occurrences_not_destinations() {
    let _guard = lock_theme_test_state();
    let mut app = app_for(
        "[a](https://same.test) [b](https://same.test) [c](#end)",
        60,
        5,
    );
    app.enter_link_mode();
    let first = app.selected_link.unwrap();
    assert_eq!(app.selected_link_index(), Some(0));
    app.move_link_focus(false);
    assert_eq!(app.selected_link_destination(), Some("#end"));
    app.move_link_focus(true);
    assert_eq!(app.selected_link, Some(first));
    app.move_link_focus(true);
    assert_ne!(app.selected_link, Some(first));
    assert_eq!(app.selected_link_destination(), Some("https://same.test"));
}

#[test]
fn link_navigation_starts_at_visible_continuation_and_never_wraps_entry() {
    let _guard = lock_theme_test_state();
    let mut app = app_for("[abcdefghij abcdefghij abcdefghij abcdefghij](https://long.test)\n\nplain\n\n[last](https://last.test)\n\nend", 20, 2);
    app.scroll_to(1);
    app.enter_link_mode();
    assert_eq!(app.selected_link_destination(), Some("https://long.test"));
    app.exit_link_mode();
    app.scroll_to(app.total().saturating_sub(1));
    app.content_area.height = 1;
    app.scroll_to(app.total().saturating_sub(1));
    let before = app.scroll();
    app.enter_link_mode();
    assert!(!app.is_link_mode());
    assert_eq!(app.scroll(), before);
    assert!(matches!(app.link_flash(), Some((LinkFlash::NoneBelow, _))));
}

#[test]
fn link_entry_reveals_tall_heading_and_exit_preserves_view() {
    let _guard = lock_theme_test_state();
    let source = format!("# {}[late](https://late.test)", "abcdefghij ".repeat(40));
    let mut app = app_for(&source, 20, 3);
    app.enter_link_mode();
    assert_eq!(app.selected_link_destination(), Some("https://late.test"));
    assert!(app.visual_scroll_offset > 0);
    let id = app.selected_link.unwrap();
    let projection = app.link_viewport_projection();
    assert!(projection.segments.iter().any(|segment| {
        app.link_spans_by_line
            .get(&segment.logical_line)
            .is_some_and(|spans| {
                spans.iter().any(|span| {
                    span.occurrence_id == id
                        && span.start_col < segment.logical_end_col
                        && span.end_col > segment.logical_start_col
                })
            })
    }));
    let anchor = (app.scroll(), app.visual_scroll_offset);
    app.exit_link_mode();
    assert_eq!((app.scroll(), app.visual_scroll_offset), anchor);
    app.scroll_top();
    assert_eq!(app.visual_scroll_offset, 0);
}

#[test]
fn link_actions_copy_exact_destination_and_gate_external_opening() {
    let _guard = lock_theme_test_state();
    let mut app = app_for(
        "[external](HTTPS://example.test/a?x=1&y=2) [internal](#part)",
        60,
        4,
    );
    app.enter_link_mode();
    let selected = app.selected_link;
    app.copy_selected_link_with(|text| {
        assert_eq!(text, "HTTPS://example.test/a?x=1&y=2");
        true
    });
    assert_eq!(app.selected_link, selected);
    assert!(matches!(app.link_flash(), Some((LinkFlash::Copied, _))));
    app.copy_selected_link_with(|_| false);
    assert!(matches!(app.link_flash(), Some((LinkFlash::CopyFailed, _))));
    app.open_selected_link_with(|text| {
        assert!(text.starts_with("HTTPS://"));
        false
    });
    assert!(matches!(app.link_flash(), Some((LinkFlash::OpenFailed, _))));
    app.move_link_focus(true);
    app.open_selected_link_with(|_| panic!("internal target must not reach opener"));
    assert!(matches!(
        app.link_flash(),
        Some((LinkFlash::UnsupportedTarget, _))
    ));
    assert!(app.is_link_mode());
}

#[test]
fn link_mode_empty_single_and_zero_viewport_are_safe() {
    let _guard = lock_theme_test_state();
    let mut empty = app_for("no links", 20, 2);
    empty.enter_link_mode();
    assert!(!empty.is_link_mode());
    assert!(matches!(empty.link_flash(), Some((LinkFlash::NoLinks, _))));
    let mut single = app_for("[one](https://one.test)", 20, 2);
    single.content_area.height = 0;
    single.enter_link_mode();
    assert!(!single.is_link_mode());
    single.content_area.height = 2;
    single.enter_link_mode();
    let id = single.selected_link;
    single.move_link_focus(true);
    single.move_link_focus(false);
    assert_eq!(single.selected_link, id);
}

#[test]
fn link_status_clips_without_wrapping_but_copies_full_destination() {
    let _guard = lock_theme_test_state();
    let destination = "https://example.test/한글/abcdefghijk?x=1234567890";
    let mut app = app_for(&format!("[long]({destination})"), 40, 3);
    app.enter_link_mode();
    app.toggle_mouse_capture();
    let narrow = draw_app(&mut app, 43, 4);
    let status: String = (0..43).map(|x| narrow[(x, 3)].symbol()).collect();
    assert!(status.contains("links.md"));
    assert!(status.contains("1/1"));
    assert!(status.contains("https://"));
    assert!(!status.contains("1234567890"));
    app.copy_selected_link_with(|text| {
        assert_eq!(text, destination);
        true
    });
    app.clear_link_flash();
    let wide = draw_app(&mut app, 180, 4);
    let mut full_status = String::new();
    let mut column = 0;
    while column < 180 {
        let symbol = wide[(column, 3)].symbol();
        full_status.push_str(symbol);
        column += crate::markdown::display_width(symbol).max(1) as u16;
    }
    assert!(full_status.contains(destination));
    assert!(full_status.contains("n/N next/prev · Enter copy · o open · f/esc cancel · q quit"));
    assert!(!full_status.contains("LINK "));
    assert!(!full_status.contains("pan"));
}

#[test]
fn document_replacement_ends_link_mode_even_when_ids_repeat() {
    let _guard = lock_theme_test_state();

    let mut app = app_for("[first](https://first.test)", 30, 3);
    app.enter_link_mode();
    let (ss, theme) = test_assets();
    let parsed = parse_markdown_with_width(
        "[second](https://second.test)",
        &ss,
        &theme,
        30,
        &test_md_theme(),
        false,
        true,
    );
    app.replace_content(parsed);
    assert!(!app.is_link_mode());
    assert_eq!(app.visual_scroll_offset, 0);
    app.enter_link_mode();
    assert_eq!(app.selected_link_destination(), Some("https://second.test"));
}

fn draw_app(app: &mut App, width: u16, height: u16) -> Buffer {
    let mut terminal = Terminal::new(TestBackend::new(width, height)).unwrap();
    terminal
        .draw(|frame| crate::render::ui(frame, app))
        .unwrap();
    terminal.backend().buffer().clone()
}

#[test]
fn selected_link_highlight_changes_only_selected_cells() {
    let _guard = lock_theme_test_state();
    let mut app = app_for(
        "[first](https://first.test) middle [second](https://second.test)",
        80,
        4,
    );
    let idle = draw_app(&mut app, 83, 5);
    app.enter_link_mode();
    let selected = app.selected_link.expect("entry selects first link");
    let focused = draw_app(&mut app, 83, 5);
    let ranges = app
        .link_spans_by_line
        .get(&0)
        .expect("link ranges on body line");
    let selected_range = ranges
        .iter()
        .find(|range| range.occurrence_id == selected)
        .expect("selected link range");
    let selected_bg = app_theme().markdown.search_match_bg;

    for y in 0..4 {
        for x in 0..83 {
            assert_eq!(
                idle.cell((x, y)).unwrap().symbol(),
                focused.cell((x, y)).unwrap().symbol(),
                "focus must not alter body glyphs at ({x},{y})"
            );
        }
    }
    for x in 1..81 {
        let logical_col = (x - 1) as usize;
        let cell = focused.cell((x, 0)).unwrap();
        let selected_cell =
            logical_col >= selected_range.start_col && logical_col < selected_range.end_col;
        assert_eq!(
            cell.bg == selected_bg,
            selected_cell,
            "only selected range should receive focus background at body col {logical_col}"
        );
    }
}

#[test]
fn narrow_unicode_link_status_renders_without_overflow() {
    let _guard = lock_theme_test_state();
    let mut app = app_for(
        "[long](https://example.test/é😀/abcdefghijk?x=1234567890)",
        40,
        3,
    );
    app.enter_link_mode();
    for width in [1, 2, 3, 8, 19, 20, 24, 40] {
        let _ = draw_app(&mut app, width, 4);
        assert!(
            app.is_link_mode(),
            "narrow status must not clear link mode at width {width}"
        );
    }
}

#[test]
fn link_status_reuses_search_palette_and_standard_section_order() {
    let _guard = lock_theme_test_state();
    let source = "[first](https://first.test) [second](https://second.test)";
    let (ss, theme) = test_assets();
    let parsed = parse_markdown_with_width(source, &ss, &theme, 80, &test_md_theme(), false, true);
    let mut app = App::new_with_source(
        Vec::new(),
        Vec::new(),
        AppConfig {
            filename: "links.md".into(),
            source: source.into(),
            debug_input: false,
            watch: true,
            filepath: None,
            last_file_state: None,
        },
    );
    app.replace_content(parsed);
    app.content_area = Rect::new(0, 0, 180, 4);
    app.enter_link_mode();
    let buffer = draw_app(&mut app, 180, 5);
    let text: String = (0..180).map(|x| buffer[(x, 4)].symbol()).collect();
    let tokens = [
        "links.md",
        "1/2",
        "watch",
        "https://first.test",
        "n/N next/prev",
        "Enter copy",
        "o open",
        "f/esc cancel",
        "q quit",
    ];
    let positions: Vec<_> = tokens
        .iter()
        .map(|token| text.find(token).unwrap())
        .collect();
    assert!(positions.windows(2).all(|pair| pair[0] < pair[1]));
    let palette = app_theme().ui;
    for (token, fg, bg) in [
        (
            "links.md",
            palette.status_filename_fg,
            palette.status_filename_bg,
        ),
        ("1/2", palette.status_success_fg, palette.status_success_bg),
        ("watch", palette.status_watch_fg, palette.status_watch_bg),
        (
            "https://first.test",
            palette.status_search_fg,
            palette.status_search_bg,
        ),
        (
            "n/N next/prev",
            palette.status_shortcut_fg,
            palette.status_bg,
        ),
    ] {
        let byte = text.find(token).unwrap();
        let column = crate::markdown::display_width(&text[..byte]) as u16;
        let cell = &buffer[(column, 4)];
        assert_eq!((cell.fg, cell.bg), (fg, bg), "section {token}");
    }
    app.move_link_focus(true);
    let changed = draw_app(&mut app, 180, 5);
    let changed_text: String = (0..180).map(|x| changed[(x, 4)].symbol()).collect();
    assert!(changed_text.contains("2/2"));
    assert!(changed_text.contains("https://second.test"));
}

#[test]
fn manual_scrolling_in_link_mode_survives_redraw_until_link_navigation() {
    let _guard = lock_theme_test_state();
    let source = format!(
        "[first](https://first.test)\n\n{}\n[last](https://last.test)",
        "padding paragraph\n\n".repeat(40)
    );
    let mut app = app_for(&source, 80, 5);
    draw_app(&mut app, 83, 6);
    app.enter_link_mode();
    let first = app.selected_link;
    app.scroll_down(20);
    let manual = app.scroll();
    assert!(manual > 0);
    draw_app(&mut app, 83, 6);
    draw_app(&mut app, 83, 6);
    assert_eq!(
        app.scroll(),
        manual,
        "redraw must not pull the view back to the selected link"
    );
    assert_eq!(app.selected_link, first);
    app.scroll_up(1);
    draw_app(&mut app, 83, 6);
    assert_eq!(app.scroll(), manual - 1);
    app.move_link_focus(true);
    draw_app(&mut app, 83, 6);
    assert_eq!(app.selected_link_destination(), Some("https://last.test"));
    assert!(app.scroll() > manual);
    app.scroll_top();
    draw_app(&mut app, 83, 6);
    assert_eq!(app.scroll(), 0);
    assert!(app.is_link_mode());
    app.move_link_focus(false);
    assert_eq!(app.selected_link, first);
}

#[test]
fn ordinary_search_and_goto_take_over_from_link_navigation() {
    let _guard = lock_theme_test_state();
    let mut app = app_for("[first](https://first.test)", 40, 4);
    app.enter_link_mode();
    app.begin_search();
    assert!(app.is_search_mode());
    assert!(!app.is_link_mode());
    app.cancel_search();
    app.enter_link_mode();
    app.begin_goto_line();
    assert!(app.is_goto_line_mode());
    assert!(!app.is_link_mode());
}

#[test]
fn editor_feedback_remains_visible_in_link_mode() {
    let _guard = lock_theme_test_state();
    let mut app = app_for("[first](https://first.test)", 80, 4);
    app.enter_link_mode();
    app.set_editor_flash(crate::app::EditorFlash::NoFile);
    let buffer = draw_app(&mut app, 160, 5);
    let status: String = (0..160).map(|x| buffer[(x, 4)].symbol()).collect();
    assert!(status.contains("No file to edit"));
    assert!(status.contains("https://first.test"));
    assert!(app.is_link_mode());
}

#[test]
fn ordinary_path_copy_feedback_is_visible_without_losing_link_selection() {
    let _guard = lock_theme_test_state();
    let mut app = app_for("[first](https://first.test)", 80, 4);
    app.enter_link_mode();
    app.set_path_flash(crate::app::PathFlash::RelativeCopied);
    let buffer = draw_app(&mut app, 160, 5);
    let status: String = (0..160).map(|x| buffer[(x, 4)].symbol()).collect();
    assert!(status.contains("Relative path copied to clipboard"));
    assert!(status.contains("https://first.test"));
    assert!(app.is_link_mode());
}
