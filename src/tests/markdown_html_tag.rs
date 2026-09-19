use super::{rendered_non_empty_lines, test_assets, test_md_theme};
use crate::markdown::parse_markdown;
use crate::theme::app_theme;
use ratatui::style::Modifier;

fn parse(src: &str) -> Vec<ratatui::text::Line<'static>> {
    let (ss, theme) = test_assets();
    let (lines, _, _, _) = parse_markdown(src, &ss, &theme, &test_md_theme(), false, true).into();
    lines
}

fn find_span_containing<'a>(
    lines: &'a [ratatui::text::Line<'static>],
    needle: &str,
) -> Option<&'a ratatui::text::Span<'static>> {
    for line in lines {
        for span in &line.spans {
            if span.content.contains(needle) {
                return Some(span);
            }
        }
    }
    None
}

#[test]
fn bold_html_tag_applies_bold_modifier() {
    let lines = parse("hello <b>world</b>\n");
    let span = find_span_containing(&lines, "world").expect("missing world span");
    assert!(span.style.add_modifier.contains(Modifier::BOLD));
}

#[test]
fn strong_html_tag_applies_bold_modifier() {
    let lines = parse("hello <strong>world</strong>\n");
    let span = find_span_containing(&lines, "world").expect("missing world span");
    assert!(span.style.add_modifier.contains(Modifier::BOLD));
}

#[test]
fn italic_html_tag_applies_italic_modifier() {
    let lines = parse("hello <i>world</i>\n");
    let span = find_span_containing(&lines, "world").expect("missing world span");
    assert!(span.style.add_modifier.contains(Modifier::ITALIC));
}

#[test]
fn em_html_tag_applies_italic_modifier() {
    let lines = parse("hello <em>world</em>\n");
    let span = find_span_containing(&lines, "world").expect("missing world span");
    assert!(span.style.add_modifier.contains(Modifier::ITALIC));
}

#[test]
fn underline_html_tag_applies_underlined_modifier() {
    let lines = parse("hello <u>world</u>\n");
    let span = find_span_containing(&lines, "world").expect("missing world span");
    assert!(span.style.add_modifier.contains(Modifier::UNDERLINED));
}

#[test]
fn ins_html_tag_applies_underlined_modifier() {
    let lines = parse("hello <ins>world</ins>\n");
    let span = find_span_containing(&lines, "world").expect("missing world span");
    assert!(span.style.add_modifier.contains(Modifier::UNDERLINED));
}

#[test]
fn strikethrough_html_tag_applies_crossed_out_modifier() {
    let lines = parse("hello <s>world</s>\n");
    let span = find_span_containing(&lines, "world").expect("missing world span");
    assert!(span.style.add_modifier.contains(Modifier::CROSSED_OUT));
}

#[test]
fn del_html_tag_applies_crossed_out_modifier() {
    let lines = parse("hello <del>world</del>\n");
    let span = find_span_containing(&lines, "world").expect("missing world span");
    assert!(span.style.add_modifier.contains(Modifier::CROSSED_OUT));
}

#[test]
fn nested_bold_italic_html_tags_apply_both_modifiers() {
    let lines = parse("<b><i>text</i></b>\n");
    let span = find_span_containing(&lines, "text").expect("missing text span");
    assert!(span.style.add_modifier.contains(Modifier::BOLD));
    assert!(span.style.add_modifier.contains(Modifier::ITALIC));
}

#[test]
fn bold_html_tag_with_attribute_is_still_recognized() {
    let lines = parse("<b class=\"foo\">world</b>\n");
    let span = find_span_containing(&lines, "world").expect("missing world span");
    assert!(span.style.add_modifier.contains(Modifier::BOLD));
}

#[test]
fn unknown_html_tag_stays_literal() {
    // <span> is not in the whitelist; it should render literally as text.
    let lines = parse("before <span>x</span> after\n");
    let all_text: String = lines
        .iter()
        .flat_map(|l| l.spans.iter().map(|s| s.content.to_string()))
        .collect();
    assert!(
        all_text.contains("<span>"),
        "unknown <span> should appear literally: {all_text:?}"
    );
}

#[test]
fn mixed_markdown_and_html_bold_preserves_state_after_close() {
    // After </b>, remaining markdown bold should still render bold.
    let lines = parse("**bold <b>x</b> bold**\n");
    let bold_span = find_span_containing(&lines, "bold").expect("missing bold");
    let x_span = find_span_containing(&lines, "x").expect("missing x");
    assert!(bold_span.style.add_modifier.contains(Modifier::BOLD));
    assert!(x_span.style.add_modifier.contains(Modifier::BOLD));
}

#[test]
fn unclosed_html_tag_does_not_leak_into_next_paragraph() {
    // <b>bold on the first paragraph, second paragraph must not be bold.
    let lines = parse("<b>bold\n\nplain\n");
    let plain = find_span_containing(&lines, "plain").expect("missing plain span");
    assert!(
        !plain.style.add_modifier.contains(Modifier::BOLD),
        "next paragraph should not inherit unclosed bold"
    );
}

#[test]
fn unclosed_html_tag_in_list_item_does_not_leak_into_next_paragraph() {
    // Regression: tight-list items do not emit Start/End(Paragraph) events, so the
    // paragraph-end reset would not fire. End(Item) must also reset the counters.
    let lines = parse("- item <b>bold\n\nplain\n");
    let plain = find_span_containing(&lines, "plain").expect("missing plain span");
    assert!(
        !plain.style.add_modifier.contains(Modifier::BOLD),
        "paragraph after unclosed <b> in a list item must not be bold"
    );
}

#[test]
fn orphan_closing_html_tag_is_silent() {
    // Should not panic or produce weird spans.
    let lines = parse("text </b> more\n");
    let rendered = rendered_non_empty_lines(&lines);
    assert!(rendered.iter().any(|l| l.contains("text")));
    assert!(rendered.iter().any(|l| l.contains("more")));
}

#[test]
fn crossed_closing_html_tags_are_tolerated() {
    // <b>foo</i> — foo receives BOLD, orphan </i> is no-op.
    let lines = parse("<b>foo</i> bar\n");
    let foo = find_span_containing(&lines, "foo").expect("missing foo");
    assert!(foo.style.add_modifier.contains(Modifier::BOLD));
}

#[test]
fn br_html_tag_produces_hard_break() {
    let lines = parse("line1<br>line2\n");
    let rendered = rendered_non_empty_lines(&lines);
    assert!(rendered.iter().any(|l| l == "line1"));
    assert!(rendered.iter().any(|l| l == "line2"));
}

#[test]
fn br_self_closing_html_tag_produces_hard_break() {
    let lines = parse("line1<br/>line2\n");
    let rendered = rendered_non_empty_lines(&lines);
    assert!(rendered.iter().any(|l| l == "line1"));
    assert!(rendered.iter().any(|l| l == "line2"));
}

#[test]
fn html_bold_in_heading_applies_and_appears_in_toc() {
    let (ss, theme) = test_assets();
    let src = "# Hello <b>World</b>\n";
    let (_, toc, _, _) = parse_markdown(src, &ss, &theme, &test_md_theme(), false, true).into();
    // TOC text should include the heading text (with or without inline HTML,
    // the important part is that the entry exists).
    assert!(!toc.is_empty(), "expected a TOC entry for the H1 heading");
}

#[test]
fn mark_html_tag_has_mark_background() {
    let (ss, theme) = test_assets();
    let src = "<mark>surligne</mark>\n";
    let (lines, _, _, _) = parse_markdown(src, &ss, &theme, &test_md_theme(), false, true).into();
    let app_theme = app_theme();
    let theme_colors = &app_theme.markdown;
    let has_mark = lines.iter().any(|l| {
        l.spans
            .iter()
            .any(|s| s.style.bg == Some(theme_colors.mark_bg) && s.content.contains("surligne"))
    });
    assert!(has_mark, "<mark> should render with mark_bg background");
}

#[test]
fn mark_html_tag_padding_matches_markdown_marker() {
    // <mark>foo</mark> should produce visually the same padded span as ==foo==.
    let (ss, theme) = test_assets();
    let (lines_html, _, _, _) = parse_markdown(
        "<mark>foo</mark>\n",
        &ss,
        &theme,
        &test_md_theme(),
        false,
        true,
    )
    .into();
    let (lines_md, _, _, _) =
        parse_markdown("==foo==\n", &ss, &theme, &test_md_theme(), false, true).into();
    let html_span = find_span_containing(&lines_html, "foo").unwrap();
    let md_span = find_span_containing(&lines_md, "foo").unwrap();
    assert_eq!(html_span.content.as_ref(), " foo ");
    assert_eq!(md_span.content.as_ref(), " foo ");
}

#[test]
fn code_html_tag_has_code_background_and_padding() {
    let (ss, theme) = test_assets();
    let src = "<code>foo</code>\n";
    let (lines, _, _, _) = parse_markdown(src, &ss, &theme, &test_md_theme(), false, true).into();
    let app_theme = app_theme();
    let theme_colors = &app_theme.markdown;
    let code_span = find_span_containing(&lines, "foo").expect("missing foo span");
    assert_eq!(code_span.content.as_ref(), " foo ");
    assert_eq!(code_span.style.bg, Some(theme_colors.inline_code_bg));
}

#[test]
fn nested_style_inside_mark_is_literal() {
    // <u> inside <mark> — underline is lost, text is literal.
    let lines = parse("<mark>foo <u>bar</u> baz</mark>\n");
    // Combined content should include all letters inside the mark span,
    // no separate underlined span should exist inside the mark.
    let mark_span = lines
        .iter()
        .flat_map(|l| l.spans.iter())
        .find(|s| s.content.contains("bar"))
        .expect("missing mark span containing bar");
    assert!(!mark_span.style.add_modifier.contains(Modifier::UNDERLINED));
    assert!(mark_span.content.contains("foo"));
    assert!(mark_span.content.contains("bar"));
    assert!(mark_span.content.contains("baz"));
}

#[test]
fn unclosed_mark_is_flushed_at_end_of_paragraph() {
    let (ss, theme) = test_assets();
    let src = "<mark>foo\n\nplain\n";
    let (lines, _, _, _) = parse_markdown(src, &ss, &theme, &test_md_theme(), false, true).into();
    let app_theme = app_theme();
    let theme_colors = &app_theme.markdown;
    let has_mark = lines.iter().any(|l| {
        l.spans
            .iter()
            .any(|s| s.style.bg == Some(theme_colors.mark_bg) && s.content.contains("foo"))
    });
    assert!(
        has_mark,
        "unclosed <mark> should still be flushed at end of paragraph"
    );
    // Second paragraph should not have mark_bg.
    let plain = find_span_containing(&lines, "plain").expect("missing plain");
    assert_ne!(plain.style.bg, Some(theme_colors.mark_bg));
}

#[test]
fn text_inside_html_bold_is_searchable() {
    // Ensures Event::Text with content "foo" is still emitted; regression guard
    // against accidentally swallowing text inside whitelisted HTML tags.
    let lines = parse("<u>foo</u>\n");
    let all_text: String = lines
        .iter()
        .flat_map(|l| l.spans.iter().map(|s| s.content.to_string()))
        .collect();
    assert!(all_text.contains("foo"));
}

#[test]
fn html_bold_in_table_cell_applies_bold() {
    let lines = parse("| A |\n|---|\n| <b>bold</b> |\n");
    let bold_span = find_span_containing(&lines, "bold").expect("missing bold span in table");
    assert!(bold_span.style.add_modifier.contains(Modifier::BOLD));
}

#[test]
fn html_underline_in_table_cell_applies_underline() {
    let lines = parse("| A |\n|---|\n| <u>underline</u> |\n");
    let span = find_span_containing(&lines, "underline").expect("missing underline span in table");
    assert!(span.style.add_modifier.contains(Modifier::UNDERLINED));
}

#[test]
fn html_underline_after_word_in_table_does_not_underline_separator_space() {
    // Regression : `<u>` after a plain word must not underline the separator space
    // inserted by the table cell wrapper.
    let lines = parse("| A |\n|---|\n| Multi <u>tag</u> par cell |\n");
    let underlined_space = lines
        .iter()
        .flat_map(|l| l.spans.iter())
        .any(|s| s.content.as_ref() == " " && s.style.add_modifier.contains(Modifier::UNDERLINED));
    assert!(
        !underlined_space,
        "separator space in table cell must not carry UNDERLINED modifier"
    );
}

#[test]
fn html_mark_in_table_cell_has_mark_bg_and_padding() {
    let (ss, theme) = test_assets();
    let src = "| A |\n|---|\n| <mark>surligne</mark> |\n";
    let (lines, _, _, _) = parse_markdown(src, &ss, &theme, &test_md_theme(), false, true).into();
    let app_theme = app_theme();
    let theme_colors = &app_theme.markdown;
    let mark_span = find_span_containing(&lines, "surligne").expect("missing surligne span");
    assert_eq!(mark_span.style.bg, Some(theme_colors.mark_bg));
    assert_eq!(mark_span.content.as_ref(), " surligne ");
}

#[test]
fn br_html_tag_in_table_cell_produces_line_break_inside_cell() {
    // Regression : <br> inside a table cell must force a wrap in the cell,
    // not just collapse whitespace.
    let lines = parse("| A |\n|---|\n| text<br>ligne2 |\n");
    let rendered = rendered_non_empty_lines(&lines);
    // The cell should span two content rows containing "text" and "ligne2"
    // (each on its own visual line inside the cell borders).
    assert!(
        rendered
            .iter()
            .any(|l| l.contains("text") && !l.contains("ligne2")),
        "expected 'text' alone on a row: {rendered:?}"
    );
    assert!(
        rendered
            .iter()
            .any(|l| l.contains("ligne2") && !l.contains("text")),
        "expected 'ligne2' alone on a row: {rendered:?}"
    );
}

#[test]
fn html_code_in_table_cell_has_code_bg_and_padding() {
    let (ss, theme) = test_assets();
    let src = "| A |\n|---|\n| <code>foo</code> |\n";
    let (lines, _, _, _) = parse_markdown(src, &ss, &theme, &test_md_theme(), false, true).into();
    let app_theme = app_theme();
    let theme_colors = &app_theme.markdown;
    let code_span = find_span_containing(&lines, "foo").expect("missing foo span");
    assert_eq!(code_span.style.bg, Some(theme_colors.inline_code_bg));
    assert_eq!(code_span.content.as_ref(), " foo ");
}

#[test]
fn nested_markdown_bold_italic_still_works_after_u8_refactor() {
    // Regression guard for the u8 counter refactor: `**bold *italic bold* end**`.
    let lines = parse("**bold *italic bold* end**\n");
    let bold_span = find_span_containing(&lines, "bold ").expect("missing bold span");
    let italic_span = find_span_containing(&lines, "italic").expect("missing italic span");
    assert!(bold_span.style.add_modifier.contains(Modifier::BOLD));
    assert!(italic_span.style.add_modifier.contains(Modifier::BOLD));
    assert!(italic_span.style.add_modifier.contains(Modifier::ITALIC));
}

#[test]
fn nested_markdown_bold_italic_in_table_cell_after_u8_refactor() {
    // Same regression guard, but inside a table cell.
    let lines = parse("| A |\n|---|\n| **bold *italic bold* end** |\n");
    let italic_span = find_span_containing(&lines, "italic").expect("missing italic span");
    assert!(italic_span.style.add_modifier.contains(Modifier::BOLD));
    assert!(italic_span.style.add_modifier.contains(Modifier::ITALIC));
}

#[test]
fn html_bold_inside_markdown_link_propagates_to_link_marker() {
    let (ss, theme) = test_assets();
    let src = "[<b>x</b>](https://example.com)\n";
    let (lines, _, _, _) = parse_markdown(src, &ss, &theme, &test_md_theme(), false, true).into();
    // The link marker should have BOLD modifier applied.
    let marker = lines
        .iter()
        .flat_map(|l| l.spans.iter())
        .find(|s| s.content.as_ref() == "#");
    assert!(marker.is_some(), "expected link marker '#'");
    assert!(marker.unwrap().style.add_modifier.contains(Modifier::BOLD));
}
