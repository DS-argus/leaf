use crate::theme::MarkdownTheme;
use ratatui::{
    style::{Modifier, Style},
    text::{Line, Span},
};
use std::ops::Range;
use unicode_segmentation::UnicodeSegmentation;

use super::width;
use super::with_link_marker;

pub(super) fn find_match_char_ranges(
    text_chars: &[char],
    query_lower: &[Vec<char>],
) -> Vec<Range<usize>> {
    let qlen = query_lower.len();
    if qlen == 0 || text_chars.len() < qlen {
        return Vec::new();
    }
    let mut ranges = Vec::new();
    let mut i = 0;
    while i + qlen <= text_chars.len() {
        let matched = text_chars[i..i + qlen]
            .iter()
            .zip(query_lower)
            .all(|(t, ql)| t.to_lowercase().eq(ql.iter().copied()));
        if matched {
            ranges.push(i..i + qlen);
            i += qlen;
        } else {
            i += 1;
        }
    }
    ranges
}

pub(super) fn content_span_start(spans: &[Span<'_>]) -> usize {
    let has_pipe = spans.iter().take(4).any(|s| s.content.contains('│'));
    if !has_pipe {
        return 0;
    }
    for (i, span) in spans.iter().enumerate() {
        if width::is_code_gutter_span(span.content.as_ref()) {
            return i + 1;
        }
    }
    0
}

fn skip_ranges_ending_by(ranges: &[Range<usize>], index: &mut usize, pos: usize) {
    while ranges.get(*index).is_some_and(|range| range.end <= pos) {
        *index += 1;
    }
}

pub(crate) fn highlight_line<'a>(line: &Line<'a>, theme: &MarkdownTheme, query: &str) -> Line<'a> {
    with_link_marker(|marker| highlight_line_with_marker(line, theme, query, marker))
}

fn highlight_line_with_marker<'a>(
    line: &Line<'a>,
    theme: &MarkdownTheme,
    query: &str,
    marker: &str,
) -> Line<'a> {
    let spans = &line.spans;
    let content_start = content_span_start(spans);

    let text_chars: Vec<char> = spans[content_start..]
        .iter()
        .filter(|s| s.content.as_ref() != marker)
        .flat_map(|s| s.content.chars())
        .collect();
    let query_lower: Vec<Vec<char>> = query.chars().map(|c| c.to_lowercase().collect()).collect();
    let ranges = find_match_char_ranges(&text_chars, &query_lower);
    if ranges.is_empty() {
        return line.clone();
    }

    let line_bg = theme.search_highlight_bg;
    let match_style_extra = Style::default()
        .bg(theme.search_match_bg)
        .add_modifier(Modifier::BOLD);
    let mut result: Vec<Span<'a>> = Vec::with_capacity(spans.len());

    for span in &spans[..content_start] {
        result.push(Span::styled(span.content.clone(), span.style.bg(line_bg)));
    }

    let mut char_offset: usize = 0;
    let mut range_index = 0;
    for span in &spans[content_start..] {
        if span.content.as_ref() == marker {
            result.push(span.clone());
            continue;
        }
        let span_len = span.content.chars().count();
        let span_end = char_offset + span_len;
        let base_style = span.style.bg(line_bg);

        skip_ranges_ending_by(&ranges, &mut range_index, char_offset);
        if ranges
            .get(range_index)
            .is_none_or(|range| range.start >= span_end)
        {
            result.push(Span::styled(span.content.clone(), base_style));
            char_offset = span_end;
            continue;
        }

        let match_style = span.style.patch(match_style_extra);
        let content = span.content.as_ref();
        let mut push_run = |text: &str, run_match: Option<usize>| {
            if !text.is_empty() {
                let style = if run_match.is_some() {
                    match_style
                } else {
                    base_style
                };
                result.push(Span::styled(text.to_string(), style));
            }
        };
        let mut run_start = 0;
        let mut run_match = None;
        let mut pos = char_offset;
        for (idx, grapheme) in content.grapheme_indices(true) {
            let end = pos + grapheme.chars().count();
            skip_ranges_ending_by(&ranges, &mut range_index, pos);
            let grapheme_match = ranges
                .get(range_index)
                .is_some_and(|range| range.start < end)
                .then_some(range_index);
            if grapheme_match != run_match {
                push_run(&content[run_start..idx], run_match);
                run_start = idx;
                run_match = grapheme_match;
            }
            pos = end;
        }
        push_run(&content[run_start..], run_match);
        char_offset = span_end;
    }

    Line::from(result)
}
