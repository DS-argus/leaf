use super::{test_assets, test_md_theme};
use crate::markdown::{
    display_width, highlight_line, parse_markdown, parse_markdown_with_width, resolve_syntax,
    LinkSpan,
};
use crate::theme::app_theme;
use ratatui::{
    style::Style,
    text::{Line, Span},
};
use syntect::parsing::SyntaxSet;

fn adversarial_table_fixture() -> (
    Vec<Line<'static>>,
    Vec<LinkSpan>,
    Vec<crate::markdown::LinkOccurrence>,
) {
    let (ss, theme) = super::test_assets();
    // A wraps in the first cell before C; parser registration is A/C/B while visual rows are A/B/C.
    let source = "| Left | Right |\n| :---: | ---: |\n| [A A A A A A A A A A A A A A A A](https://example.test/a) [C](https://example.test/c) | [B](https://example.test/b) |\n";
    let parsed = parse_markdown_with_width(
        source,
        &ss,
        &theme,
        36,
        &super::test_md_theme(),
        false,
        true,
    );
    (parsed.lines, parsed.link_spans, parsed.link_occurrences)
}

fn span_destinations<'a>(
    spans: &[LinkSpan],
    occurrences: &'a [crate::markdown::LinkOccurrence],
) -> Vec<&'a str> {
    spans
        .iter()
        .map(|span| occurrences[span.occurrence_id.0].destination.as_str())
        .collect()
}

fn table_link_marker_positions(lines: &[Line<'_>]) -> Vec<(usize, usize, char)> {
    let link_icon = app_theme().markdown.link_icon;
    lines
        .iter()
        .enumerate()
        .flat_map(|(line_idx, line)| {
            let text: String = line
                .spans
                .iter()
                .map(|span| span.content.as_ref())
                .collect();
            let mut col = 0usize;
            let mut markers = Vec::new();
            for span in &line.spans {
                let content = span.content.as_ref();
                if content == "#" && span.style.fg == Some(link_icon) {
                    let label = text
                        .chars()
                        .nth(col + 1)
                        .expect("fixture marker should be followed by a label");
                    markers.push((line_idx, col, label));
                }
                col += display_width(content);
            }
            markers
        })
        .collect()
}

#[test]
fn blockquote_bold_link_preserves_link_color() {
    let (ss, theme) = test_assets();
    let src = "> text [**lien bold**](https://rivolink.mg)\n";
    let (lines, _, _, _) = parse_markdown(src, &ss, &theme, &test_md_theme(), false, true).into();
    let app_theme = app_theme();
    let theme_colors = &app_theme.markdown;

    let bq_line = &lines[0];
    let link_span = bq_line.spans.iter().find(|s| s.content.contains("lien"));
    assert!(link_span.is_some(), "should find 'lien' span");
    let span = link_span.unwrap();
    assert_eq!(
        span.style.fg,
        Some(theme_colors.link_text),
        "bold link in blockquote should preserve link_text color"
    );
}

#[test]
fn link_spans_detected_for_all_link_types() {
    let (ss, theme) = test_assets();
    let md = "\
[Simple](https://example.com/simple)

**[Bold link](https://example.com/bold)**

*[Italic link](https://example.com/italic)*

~~[Strike link](https://example.com/strike)~~

[Internal](#section)

### [Heading link](https://example.com/heading)

> [Blockquote link](https://example.com/quote)

[A](https://example.com/a) and [B](https://example.com/b)
";
    let parsed = parse_markdown(md, &ss, &theme, &test_md_theme(), false, true);
    let urls = span_destinations(&parsed.link_spans, &parsed.link_occurrences);

    for expected in [
        "https://example.com/simple",
        "https://example.com/bold",
        "https://example.com/italic",
        "https://example.com/strike",
        "#section",
        "https://example.com/heading",
        "https://example.com/quote",
        "https://example.com/a",
        "https://example.com/b",
    ] {
        assert!(
            urls.contains(&expected),
            "link missing: {expected}, got {urls:?}"
        );
    }
    for span in &parsed.link_spans {
        assert!(span.end_col > span.start_col);
    }
}

#[test]
fn link_spans_in_table_are_detected() {
    let (ss, theme) = test_assets();
    let md = "\
| Name | Link |
|------|------|
| Test | [example](https://example.com/table) |
";
    let parsed = parse_markdown(md, &ss, &theme, &test_md_theme(), false, true);
    let urls = span_destinations(&parsed.link_spans, &parsed.link_occurrences);
    assert!(
        urls.contains(&"https://example.com/table"),
        "table link missing: {urls:?}"
    );
}

#[test]
fn wrapped_repeated_destinations_keep_distinct_ids_and_continuations() {
    let (ss, theme) = test_assets();
    let md = "[one two three four five six seven](https://example.test/repeat) and [again](https://example.test/repeat)";
    let parsed = parse_markdown_with_width(md, &ss, &theme, 18, &test_md_theme(), false, true);
    assert_eq!(parsed.link_occurrences.len(), 2);
    assert_ne!(parsed.link_occurrences[0].id, parsed.link_occurrences[1].id);
    assert_eq!(
        parsed.link_occurrences[0].destination,
        parsed.link_occurrences[1].destination
    );
    let first_ranges: Vec<_> = parsed
        .link_spans
        .iter()
        .filter(|span| span.occurrence_id == parsed.link_occurrences[0].id)
        .collect();
    assert!(
        first_ranges.len() > 1,
        "wrapped label should have multiple ranges"
    );
    assert!(first_ranges
        .iter()
        .all(|span| span.end_col > span.start_col));
}

#[test]
fn mixed_style_mapping_does_not_depend_on_link_color() {
    let (ss, theme) = test_assets();
    let mut md_theme = test_md_theme();
    md_theme.link_text = md_theme.text;
    let parsed = parse_markdown(
        "[**bold** and `code` and ==mark==](https://example.test/mixed)",
        &ss,
        &theme,
        &md_theme,
        false,
        true,
    );
    assert_eq!(parsed.link_occurrences.len(), 1);
    assert!(!parsed.link_spans.is_empty());
    assert!(parsed
        .link_spans
        .iter()
        .all(|span| span.occurrence_id == parsed.link_occurrences[0].id));
}

#[test]
fn highlight_line_single_match() {
    let theme = test_md_theme();
    let line_bg = theme.search_highlight_bg;
    let match_bg = theme.search_match_bg;
    let line = Line::from(vec![Span::raw("hello world")]);
    let result = highlight_line(&line, &theme, "world");
    assert_eq!(result.spans.len(), 2);
    assert_eq!(result.spans[0].content.as_ref(), "hello ");
    assert_eq!(result.spans[0].style.bg, Some(line_bg));
    assert_eq!(result.spans[1].content.as_ref(), "world");
    assert_eq!(result.spans[1].style.bg, Some(match_bg));
    assert!(result.spans[1]
        .style
        .add_modifier
        .contains(ratatui::style::Modifier::BOLD));
}

#[test]
fn highlight_line_multiple_matches() {
    let theme = test_md_theme();
    let match_bg = theme.search_match_bg;
    let line = Line::from(vec![Span::raw("abcabcabc")]);
    let result = highlight_line(&line, &theme, "abc");
    assert_eq!(result.spans.len(), 3);
    for span in &result.spans {
        assert_eq!(span.content.as_ref(), "abc");
        assert_eq!(span.style.bg, Some(match_bg));
        assert!(span
            .style
            .add_modifier
            .contains(ratatui::style::Modifier::BOLD));
    }
}

#[test]
fn highlight_line_case_insensitive() {
    let theme = test_md_theme();
    let line_bg = theme.search_highlight_bg;
    let match_bg = theme.search_match_bg;
    let line = Line::from(vec![Span::raw("Hello World")]);
    let result = highlight_line(&line, &theme, "hello");
    assert_eq!(result.spans.len(), 2);
    assert_eq!(result.spans[0].content.as_ref(), "Hello");
    assert_eq!(result.spans[0].style.bg, Some(match_bg));
    assert!(result.spans[0]
        .style
        .add_modifier
        .contains(ratatui::style::Modifier::BOLD));
    assert_eq!(result.spans[1].content.as_ref(), " World");
    assert_eq!(result.spans[1].style.bg, Some(line_bg));
    assert!(!result.spans[1]
        .style
        .add_modifier
        .contains(ratatui::style::Modifier::BOLD));
}

#[test]
fn highlight_line_cross_span() {
    let theme = test_md_theme();
    let line_bg = theme.search_highlight_bg;
    let match_bg = theme.search_match_bg;
    let bold = Style::default().add_modifier(ratatui::style::Modifier::BOLD);
    let line = Line::from(vec![Span::styled("hel", bold), Span::raw("lo world")]);
    let result = highlight_line(&line, &theme, "hello");
    assert_eq!(result.spans[0].content.as_ref(), "hel");
    assert_eq!(result.spans[0].style.bg, Some(match_bg));
    assert!(result.spans[0]
        .style
        .add_modifier
        .contains(ratatui::style::Modifier::BOLD));
    assert_eq!(result.spans[1].content.as_ref(), "lo");
    assert_eq!(result.spans[1].style.bg, Some(match_bg));
    assert!(result.spans[1]
        .style
        .add_modifier
        .contains(ratatui::style::Modifier::BOLD));
    assert_eq!(result.spans[2].content.as_ref(), " world");
    assert_eq!(result.spans[2].style.bg, Some(line_bg));
}

#[test]
fn highlight_line_no_match_returns_clone() {
    let theme = test_md_theme();
    let line = Line::from(vec![Span::raw("hello world")]);
    let result = highlight_line(&line, &theme, "xyz");
    assert_eq!(result.spans.len(), 1);
    assert_eq!(result.spans[0].content.as_ref(), "hello world");
    assert_eq!(result.spans[0].style.bg, None);
}

#[test]
fn resolve_syntax_supports_common_language_aliases() {
    let ss = SyntaxSet::load_defaults_newlines();

    assert_eq!(
        resolve_syntax("py", &ss).name,
        resolve_syntax("python", &ss).name
    );
    assert_eq!(
        resolve_syntax("cpp", &ss).name,
        resolve_syntax("c++", &ss).name
    );
    assert_eq!(resolve_syntax("json", &ss).name, "JSON");
    assert_eq!(resolve_syntax("json5", &ss).name, "JSON");
    assert_eq!(
        resolve_syntax("ps1", &ss).name,
        resolve_syntax("powershell", &ss).name
    );

    for tag in &["kotlin", "toml", "jsx", "dockerfile"] {
        assert_ne!(
            resolve_syntax(tag, &ss).name,
            "Plain Text",
            "{tag} should not fall back to Plain Text"
        );
    }
    assert_eq!(
        resolve_syntax("kt", &ss).name,
        resolve_syntax("kotlin", &ss).name
    );
    assert_eq!(
        resolve_syntax("docker", &ss).name,
        resolve_syntax("dockerfile", &ss).name
    );
    assert_eq!(
        resolve_syntax("pwsh", &ss).name,
        resolve_syntax("ps1", &ss).name
    );
}

#[test]
fn resolve_syntax_php_uses_php_source() {
    let ss = SyntaxSet::load_defaults_newlines();

    for tag in &["php", "PHP", "php3", "php4", "php5", "php7", "phtml"] {
        assert_eq!(
            resolve_syntax(tag, &ss).name,
            "PHP Source",
            "{tag} should resolve to PHP Source"
        );
    }
}

#[test]
fn php_code_block_without_open_tag_is_highlighted() {
    let (ss, theme) = test_assets();
    let md = "```php\nforeach ($map as $item) {\n    $scores[] = $item ?? 0;\n}\n```\n";
    let (lines, _, _, _) = parse_markdown(md, &ss, &theme, &test_md_theme(), false, true).into();

    let span_fg = |needle: &str| {
        lines
            .iter()
            .flat_map(|l| l.spans.iter())
            .find(|s| s.content.contains(needle))
            .and_then(|s| s.style.fg)
    };
    let keyword_fg = span_fg("foreach");
    let variable_fg = span_fg("scores");

    assert!(keyword_fg.is_some(), "should find 'foreach' span");
    assert!(variable_fg.is_some(), "should find 'scores' span");
    assert_ne!(
        keyword_fg, variable_fg,
        "php block without <?php should highlight keywords and variables differently"
    );
}

#[test]
fn adversarial_table_fixture_preserves_visual_destination_order() {
    const A: &str = "https://example.test/a";
    const B: &str = "https://example.test/b";
    const C: &str = "https://example.test/c";

    let (lines, link_spans, occurrences) = adversarial_table_fixture();
    let markers = table_link_marker_positions(&lines);
    let labels: Vec<char> = markers.iter().map(|(_, _, label)| *label).collect();
    assert_eq!(labels, vec!['A', 'B', 'C']);
    assert!(
        markers[2].0 > markers[0].0,
        "C follows A on a continuation row"
    );

    for ((line, column, _), destination) in markers.iter().zip([A, B, C]) {
        let range = link_spans
            .iter()
            .find(|span| {
                span.line_idx == *line && span.start_col <= *column && *column < span.end_col
            })
            .expect("every visible marker must have exact ownership");
        assert_eq!(occurrences[range.occurrence_id.0].destination, destination);
    }
    let mut seen = std::collections::HashSet::new();
    let visual_destinations: Vec<_> = link_spans
        .iter()
        .filter(|span| seen.insert(span.occurrence_id))
        .map(|span| occurrences[span.occurrence_id.0].destination.as_str())
        .collect();
    assert_eq!(visual_destinations, vec![A, B, C]);
    assert!(
        link_spans
            .iter()
            .filter(|span| occurrences[span.occurrence_id.0].destination == A)
            .count()
            > 1
    );
    assert_eq!(
        occurrences
            .iter()
            .map(|occurrence| occurrence.destination.as_str())
            .collect::<Vec<_>>(),
        vec![A, C, B]
    );
}
