use crate::{
    diff::{DiffFile, HunkLine},
    theme::DiffTheme,
};
use ratatui::{
    style::{Color, Modifier, Style},
    text::{Line, Span},
};
use syntect::{
    easy::HighlightLines,
    highlighting::FontStyle,
    parsing::{SyntaxReference, SyntaxSet},
};

pub(super) struct BodyLineStyle<'a> {
    pub(super) theme: &'a DiffTheme,
    pub(super) fg: Color,
    pub(super) bg: Option<Color>,
    pub(super) intra_bg: Option<Color>,
}

pub(super) fn build_body_line(
    style: &BodyLineStyle<'_>,
    text: &str,
    hl: Option<&mut HighlightLines<'_>>,
    ss: &SyntaxSet,
    intra_mask: Option<&Vec<bool>>,
) -> Line<'static> {
    let BodyLineStyle {
        theme,
        fg,
        bg,
        intra_bg,
    } = *style;
    let mut fallback_style = Style::default().fg(fg);
    if let Some(bg) = bg {
        fallback_style = fallback_style.bg(bg);
    }

    let (sigil_char, sigil_style, sep_style) = if let Some(bg) = bg {
        let (ch, style) = if bg == theme.add_bg {
            ('+', Style::default().fg(theme.add_fg).bg(theme.add_bg))
        } else if bg == theme.del_bg {
            ('-', Style::default().fg(theme.del_fg).bg(theme.del_bg))
        } else {
            (' ', Style::default().bg(bg))
        };
        (ch, style, Style::default().bg(bg))
    } else {
        (' ', Style::default(), Style::default())
    };
    let border_style = Style::default().fg(theme.frame_fg);
    let mut spans: Vec<Span<'static>> = vec![
        Span::styled("│", border_style),
        Span::styled(sigil_char.to_string(), sigil_style),
        Span::styled(" ", sep_style),
    ];

    let effective_mask: Option<&Vec<bool>> = match (intra_mask, bg, intra_bg) {
        (Some(m), Some(_), Some(_)) if m.len() == text.chars().count() => Some(m),
        _ => None,
    };

    if let Some(hl) = hl {
        let mut input = String::with_capacity(text.len() + 1);
        input.push_str(text);
        input.push('\n');
        if let Ok(regions) = hl.highlight_line(&input, ss) {
            let mut content: Vec<Span<'static>> = Vec::new();
            for (st, chunk) in regions {
                let s = chunk.trim_end_matches('\n');
                if s.is_empty() {
                    continue;
                }
                let mut base_style = Style::default().fg(syntect_to_color(st.foreground));
                if let Some(bg) = bg {
                    base_style = base_style.bg(bg);
                }
                if st.font_style.contains(FontStyle::BOLD) {
                    base_style = base_style.add_modifier(Modifier::BOLD);
                }
                if st.font_style.contains(FontStyle::ITALIC) {
                    base_style = base_style.add_modifier(Modifier::ITALIC);
                }
                if st.font_style.contains(FontStyle::UNDERLINE) {
                    base_style = base_style.add_modifier(Modifier::UNDERLINED);
                }
                content.push(Span::styled(s.to_string(), base_style));
            }
            if let (Some(mask), Some(ibg)) = (effective_mask, intra_bg) {
                content = apply_intra_mask(content, mask, ibg);
            }
            spans.extend(content);
            return Line::from(spans);
        }
    }

    let plain = vec![Span::styled(text.to_string(), fallback_style)];
    match (effective_mask, intra_bg, bg) {
        (Some(mask), Some(ibg), Some(_)) => spans.extend(apply_intra_mask(plain, mask, ibg)),
        _ => spans.extend(plain),
    }
    Line::from(spans)
}

const CR_MARKER: char = '␍';

pub(crate) fn display_text(raw: &str, start_width: usize) -> (String, bool) {
    let (text, had_cr) = sanitize_text(raw);
    if !text.contains('\t') {
        return (text, had_cr);
    }
    (
        crate::markdown::width::expand_tabs(&text, start_width),
        had_cr,
    )
}

pub(crate) fn sanitize_text(raw: &str) -> (String, bool) {
    let (body, had_cr) = match raw.strip_suffix('\r') {
        Some(body) => (body, true),
        None => (raw, false),
    };
    if !body.contains(|c: char| c.is_control() && c != '\t') {
        let mut out = String::with_capacity(body.len() + 1);
        out.push_str(body);
        return (out, had_cr);
    }
    let mut out = String::with_capacity(body.len() + 4);
    for c in body.chars() {
        match c {
            '\t' => out.push(c),
            c if (c as u32) < 0x20 => {
                out.push('^');
                out.push(char::from(c as u8 + 0x40));
            }
            '\u{7f}' => out.push_str("^?"),
            c if c.is_control() => out.push('\u{fffd}'),
            c => out.push(c),
        }
    }
    (out, had_cr)
}

pub(super) fn expand_segments(
    segments: &[crate::diff::WordDiffSegment],
    keep: impl Fn(&crate::diff::WordDiffKind) -> bool,
) -> Vec<(&crate::diff::WordDiffKind, String)> {
    let mut width = 0;
    segments
        .iter()
        .filter(|seg| keep(&seg.kind))
        .map(|seg| {
            let text = crate::markdown::width::expand_tabs(&seg.text, width);
            width += crate::markdown::display_width(&text);
            (&seg.kind, text)
        })
        .collect()
}

pub(super) fn cr_marker_mask(text: &str) -> Option<Vec<bool>> {
    if !text.ends_with(CR_MARKER) {
        return None;
    }
    let len = text.chars().count();
    Some((0..len).map(|i| i + 1 < len).collect())
}

pub(super) fn prepare_hunk_lines(lines: &[HunkLine]) -> Vec<HunkLine> {
    let mut crs = vec![false; lines.len()];
    let mut out: Vec<HunkLine> = lines
        .iter()
        .zip(crs.iter_mut())
        .map(|(line, cr)| match line {
            HunkLine::Context(s) => HunkLine::Context(display_text(s, 0).0),
            HunkLine::Del(s) => {
                let (text, had_cr) = display_text(s, 0);
                *cr = had_cr;
                HunkLine::Del(text)
            }
            HunkLine::Add(s) => {
                let (text, had_cr) = display_text(s, 0);
                *cr = had_cr;
                HunkLine::Add(text)
            }
            HunkLine::Combined(prefix, s) => {
                HunkLine::Combined(prefix.clone(), display_text(s, 0).0)
            }
            HunkLine::WordDiff(segments) => HunkLine::WordDiff(
                segments
                    .iter()
                    .map(|seg| crate::diff::WordDiffSegment {
                        kind: seg.kind.clone(),
                        text: sanitize_text(&seg.text).0,
                    })
                    .collect(),
            ),
            HunkLine::NoNewline => HunkLine::NoNewline,
        })
        .collect();
    for (del_idx, add_idx) in change_pairs(lines) {
        if crs[del_idx] != crs[add_idx] {
            let idx = if crs[del_idx] { del_idx } else { add_idx };
            if let HunkLine::Del(text) | HunkLine::Add(text) = &mut out[idx] {
                text.push(CR_MARKER);
            }
        }
    }
    out
}

pub(super) fn change_pairs(lines: &[HunkLine]) -> Vec<(usize, usize)> {
    let mut pairs = Vec::new();
    let mut i = 0;
    while i < lines.len() {
        if !matches!(lines[i], HunkLine::Del(_)) {
            i += 1;
            continue;
        }
        let del_start = i;
        while i < lines.len() && matches!(lines[i], HunkLine::Del(_)) {
            i += 1;
        }
        let del_end = i;
        while i < lines.len() && matches!(lines[i], HunkLine::NoNewline) {
            i += 1;
        }
        let add_start = i;
        while i < lines.len() && matches!(lines[i], HunkLine::Add(_)) {
            i += 1;
        }
        let pair_count = (del_end - del_start).min(i - add_start);
        pairs.extend((0..pair_count).map(|k| (del_start + k, add_start + k)));
    }
    pairs
}

pub(super) type IntraMaskPair = (Option<Vec<bool>>, Option<Vec<bool>>);

pub(super) fn compute_intra_masks(lines: &[HunkLine]) -> Vec<IntraMaskPair> {
    let mut out: Vec<IntraMaskPair> = vec![(None, None); lines.len()];
    for (del_idx, add_idx) in change_pairs(lines) {
        let (HunkLine::Del(a), HunkLine::Add(b)) = (&lines[del_idx], &lines[add_idx]) else {
            continue;
        };
        if a.is_empty() || b.is_empty() {
            continue;
        }
        if let Some((mask_a, mask_b)) = char_diff_masks(a, b) {
            out[del_idx].0 = Some(mask_a);
            out[add_idx].1 = Some(mask_b);
        }
    }
    for (line, masks) in lines.iter().zip(out.iter_mut()) {
        match line {
            HunkLine::Del(s) if masks.0.is_none() => masks.0 = cr_marker_mask(s),
            HunkLine::Add(s) if masks.1.is_none() => masks.1 = cr_marker_mask(s),
            _ => {}
        }
    }
    out
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum TokClass {
    Word,
    Space,
    Other,
}

pub(super) fn classify(c: char) -> TokClass {
    if c.is_alphanumeric() || c == '_' {
        TokClass::Word
    } else if c.is_whitespace() {
        TokClass::Space
    } else {
        TokClass::Other
    }
}

pub(super) fn tokenize(s: &str) -> Vec<(usize, &str)> {
    let mut out: Vec<(usize, &str)> = Vec::new();
    let bytes = s.as_bytes();
    let mut it = s.char_indices().peekable();
    while let Some((start, c)) = it.next() {
        let cls = classify(c);
        if matches!(cls, TokClass::Other) {
            let end = start + c.len_utf8();
            out.push((start, &s[start..end]));
            continue;
        }
        let mut end = start + c.len_utf8();
        while let Some(&(_, nc)) = it.peek() {
            if classify(nc) == cls {
                let (ni, nch) = it.next().unwrap();
                end = ni + nch.len_utf8();
            } else {
                break;
            }
        }

        debug_assert!(end <= bytes.len());
        out.push((start, &s[start..end]));
    }
    out
}

const MAX_LCS_CELLS: usize = 250_000;

pub(super) fn char_diff_masks(a: &str, b: &str) -> Option<(Vec<bool>, Vec<bool>)> {
    let a_char_len = a.chars().count();
    let b_char_len = b.chars().count();
    if a_char_len == 0 || b_char_len == 0 {
        return None;
    }
    let ta = tokenize(a);
    let tb = tokenize(b);

    let max_prefix = ta.len().min(tb.len());
    let mut prefix = 0usize;
    while prefix < max_prefix && ta[prefix].1 == tb[prefix].1 {
        prefix += 1;
    }
    let mut suffix = 0usize;
    while suffix < (ta.len() - prefix).min(tb.len() - prefix)
        && ta[ta.len() - 1 - suffix].1 == tb[tb.len() - 1 - suffix].1
    {
        suffix += 1;
    }

    let a_mid = &ta[prefix..ta.len() - suffix];
    let b_mid = &tb[prefix..tb.len() - suffix];

    let count_chars =
        |toks: &[(usize, &str)]| -> usize { toks.iter().map(|(_, s)| s.chars().count()).sum() };
    let a_pre_chars = count_chars(&ta[..prefix]);
    let b_pre_chars = count_chars(&tb[..prefix]);
    let a_mid_chars = count_chars(a_mid);
    let b_mid_chars = count_chars(b_mid);

    if a_mid.is_empty() && b_mid.is_empty() {
        return None;
    }

    let m = a_mid.len();
    let n = b_mid.len();
    let mut token_mask_a = vec![false; m];
    let mut token_mask_b = vec![false; n];

    if m > 0 && n > 0 && m.saturating_mul(n) <= MAX_LCS_CELLS {
        let mut dp = vec![vec![0u32; n + 1]; m + 1];
        for i in 0..m {
            for j in 0..n {
                dp[i + 1][j + 1] = if a_mid[i].1 == b_mid[j].1 {
                    dp[i][j] + 1
                } else {
                    dp[i + 1][j].max(dp[i][j + 1])
                };
            }
        }

        let (mut i, mut j) = (m, n);
        let mut lcs_chars: usize = 0;
        while i > 0 && j > 0 {
            if a_mid[i - 1].1 == b_mid[j - 1].1 {
                token_mask_a[i - 1] = true;
                token_mask_b[j - 1] = true;
                lcs_chars += a_mid[i - 1].1.chars().count();
                i -= 1;
                j -= 1;
            } else if dp[i - 1][j] >= dp[i][j - 1] {
                i -= 1;
            } else {
                j -= 1;
            }
        }

        let trimmed_common_a = a_char_len - a_mid_chars;
        let trimmed_common_b = b_char_len - b_mid_chars;
        let common_shared = trimmed_common_a.min(trimmed_common_b) + lcs_chars;
        let shorter_full = a_char_len.min(b_char_len);
        if shorter_full > 0 && (common_shared * 100) < (shorter_full * 30) {
            return None;
        }
    }

    let mut mask_a = vec![false; a_char_len];
    let mut mask_b = vec![false; b_char_len];

    mask_a[..a_pre_chars].fill(true);
    mask_b[..b_pre_chars].fill(true);

    let mut cursor = a_pre_chars;
    for (i, (_, tok)) in a_mid.iter().enumerate() {
        let clen = tok.chars().count();
        if token_mask_a[i] {
            mask_a[cursor..cursor + clen].fill(true);
        }
        cursor += clen;
    }
    let mut cursor = b_pre_chars;
    for (i, (_, tok)) in b_mid.iter().enumerate() {
        let clen = tok.chars().count();
        if token_mask_b[i] {
            mask_b[cursor..cursor + clen].fill(true);
        }
        cursor += clen;
    }

    let a_suf_chars = a_char_len - a_pre_chars - a_mid_chars;
    let b_suf_chars = b_char_len - b_pre_chars - b_mid_chars;
    mask_a[a_char_len - a_suf_chars..].fill(true);
    mask_b[b_char_len - b_suf_chars..].fill(true);

    Some((mask_a, mask_b))
}

pub(crate) fn syntect_to_color(c: syntect::highlighting::Color) -> Color {
    Color::Rgb(c.r, c.g, c.b)
}

pub(crate) fn resolve_file_syntax<'a>(
    file: &DiffFile,
    ss: &'a SyntaxSet,
) -> Option<&'a SyntaxReference> {
    let candidates = [file.new_path.as_str(), file.old_path.as_str()];
    for path in candidates {
        if path.is_empty() || path == "/dev/null" {
            continue;
        }
        let path = std::path::Path::new(path);
        if let Some(ext) = path.extension().and_then(|e| e.to_str()) {
            if let Some(sx) = ss.find_syntax_by_extension(ext) {
                return Some(sx);
            }
        }
        if let Some(name) = path.file_name().and_then(|n| n.to_str()) {
            if let Some(sx) = ss.find_syntax_by_extension(name) {
                return Some(sx);
            }
        }
    }
    file.hunks
        .iter()
        .find(|hunk| hunk.header.new_start <= 1 || hunk.header.old_start <= 1)
        .and_then(|hunk| {
            hunk.lines.iter().find_map(|line| match line {
                HunkLine::Context(s) | HunkLine::Add(s) | HunkLine::Del(s) => Some(s.as_str()),
                _ => None,
            })
        })
        .and_then(|first| ss.find_syntax_by_first_line(first))
}

pub(super) fn apply_intra_mask(
    spans: Vec<Span<'static>>,
    mask: &[bool],
    intra_bg: Color,
) -> Vec<Span<'static>> {
    let mut out: Vec<Span<'static>> = Vec::with_capacity(spans.len() + 4);
    let mut cursor = 0usize;
    for span in spans {
        let is_common = |k: usize| mask.get(k).copied().unwrap_or(true);
        let style_of = |common: bool| {
            if common {
                span.style
            } else {
                span.style.bg(intra_bg)
            }
        };
        let chars = span.content.chars().count();
        let first = is_common(cursor);
        if (1..chars).all(|k| is_common(cursor + k) == first) {
            let style = style_of(first);
            if chars > 0 {
                out.push(span.style(style));
            }
            cursor += chars;
            continue;
        }
        let bounds: Vec<usize> = span
            .content
            .char_indices()
            .map(|(b, _)| b)
            .chain(std::iter::once(span.content.len()))
            .collect();
        let mut i = 0;
        while i < chars {
            let common = is_common(cursor + i);
            let mut j = i + 1;
            while j < chars && is_common(cursor + j) == common {
                j += 1;
            }
            out.push(Span::styled(
                span.content[bounds[i]..bounds[j]].to_string(),
                style_of(common),
            ));
            i = j;
        }
        cursor += chars;
    }
    out
}
