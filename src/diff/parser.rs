use super::{
    strip_prefix, DiffFile, FileChange, Hunk, HunkHeader, HunkLine, WordDiffKind, WordDiffSegment,
};

const GITLINK_MODE: &str = "160000";
const MAX_HUNK_PREALLOC_LINES: usize = 4096;

pub(crate) fn parse_unified_diff(src: &str) -> Vec<DiffFile> {
    let mut parser = Parser {
        word_diff: word_diff_format(src),
        ..Parser::default()
    };
    for line in src.split_inclusive('\n') {
        parser.push_line(line.strip_suffix('\n').unwrap_or(line));
    }
    parser.finish()
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
enum WordDiffFormat {
    #[default]
    None,
    Plain,
    Porcelain,
}

fn word_diff_format(src: &str) -> WordDiffFormat {
    let lines: Vec<&str> = src.lines().collect();
    let (mut old_left, mut new_left) = (0u32, 0u32);
    let mut after_hunk_line = false;
    let (mut has_change, mut has_markers) = (false, false);
    for (i, &line) in lines.iter().enumerate() {
        let ends_hunk = old_left == 1 && new_left == 1 && line.starts_with("--- ");
        let file_header = !ends_hunk && is_file_header_at(&lines, i);
        if file_header || line.starts_with("@@") {
            if has_markers && (old_left > 0 || new_left > 0) {
                return WordDiffFormat::Plain;
            }
            (old_left, new_left) = (0, 0);
            (has_change, has_markers, after_hunk_line) = (false, false, false);
            if !file_header && !line.starts_with("@@@") {
                if let Some((header, _)) = parse_hunk_header(line) {
                    (old_left, new_left) = (header.old_count, header.new_count);
                }
            }
            continue;
        }
        if line == "~" && (old_left > 0 || new_left > 0 || after_hunk_line) {
            return WordDiffFormat::Porcelain;
        }
        if old_left == 0 && new_left == 0 {
            after_hunk_line &= matches!(line.chars().next(), Some('+' | '-' | ' '));
            continue;
        }
        after_hunk_line = true;
        has_markers = has_markers || has_word_diff_markers(line);
        match line.chars().next() {
            Some('+') => {
                new_left = new_left.saturating_sub(1);
                has_change = true;
            }
            Some('-') => {
                old_left = old_left.saturating_sub(1);
                has_change = true;
            }
            Some(' ') | None => {
                old_left = old_left.saturating_sub(1);
                new_left = new_left.saturating_sub(1);
            }
            Some('\\') => {}
            Some(_) => return WordDiffFormat::Plain,
        }
        if old_left == 0 && new_left == 0 {
            if has_markers && !has_change {
                return WordDiffFormat::Plain;
            }
            (has_change, has_markers) = (false, false);
        }
    }
    if has_markers && (old_left > 0 || new_left > 0) {
        return WordDiffFormat::Plain;
    }
    WordDiffFormat::None
}

fn is_file_header_at(lines: &[&str], i: usize) -> bool {
    let line = lines[i];
    let next = |k: usize| lines.get(i + k).copied().unwrap_or_default();
    super::GIT_FILE_HEADER_PREFIXES
        .iter()
        .any(|prefix| line.starts_with(prefix))
        || is_submodule_marker_line(line)
        || (line.starts_with("--- ") && next(1).starts_with("+++ ") && next(2).starts_with("@@"))
}

fn has_word_diff_markers(line: &str) -> bool {
    try_parse_word_diff(line).is_some_and(|segments| {
        segments
            .iter()
            .any(|segment| segment.kind != WordDiffKind::Common)
    })
}

#[derive(Default)]
struct Parser {
    word_diff: WordDiffFormat,
    porcelain_segments: Vec<WordDiffSegment>,
    pending_plain_header: Option<String>,
    files: Vec<DiffFile>,
    current: Option<DiffFile>,
    current_hunk: Option<Hunk>,
    hunk_old_left: u32,
    hunk_new_left: u32,
    submodule_nested_buf: Option<String>,
    submodule_prefix: String,
    submodule_prefixed: Option<bool>,
    pending_plain_old: Option<String>,
}

impl Parser {
    fn push_line(&mut self, raw: &str) {
        let line = raw.strip_suffix('\r').unwrap_or(raw);

        if let Some(buf) = self.submodule_nested_buf.as_mut() {
            let prefix = &self.submodule_prefix;
            let other_submodule = is_submodule_marker_line(line)
                && !submodule_marker_path(line).starts_with(prefix.as_str());
            let nested = match line.strip_prefix("diff --git ") {
                None => !other_submodule,
                Some(rest) => {
                    let prefixed = strip_prefix(rest).starts_with(prefix.as_str());
                    match self.submodule_prefixed {
                        None if prefixed || buf.is_empty() => {
                            self.submodule_prefixed = Some(prefixed);
                            true
                        }
                        None => false,
                        Some(true) => prefixed,
                        Some(false) => true,
                    }
                }
            };
            if nested {
                buf.push_str(raw);
                buf.push('\n');
                return;
            }
            self.flush_file();
        }

        if is_submodule_marker_line(line) {
            self.open_submodule(line);
            return;
        }

        if let Some(rest) = line.strip_prefix("diff --git ") {
            self.flush_hunk();
            self.flush_file();
            let (a, b) = split_diff_git_paths(rest);
            self.pending_plain_header = None;
            self.current = Some(DiffFile::new(a, b, line));
            return;
        }

        if let Some(rest) = line
            .strip_prefix("diff --cc ")
            .or_else(|| line.strip_prefix("diff --combined "))
        {
            self.flush_hunk();
            self.flush_file();
            let path = decode_path(rest);
            self.pending_plain_header = None;
            self.current = Some(DiffFile::new(path.clone(), path, line));
            return;
        }

        if self.current_hunk.is_some() && self.push_hunk_line(raw) {
            return;
        }

        if is_plain_diff_command(line) && self.current_hunk.is_none() {
            self.flush_file();
            self.pending_plain_header = Some(line.to_string());
            return;
        }

        if self.try_plain_file_header(line) {
            return;
        }

        if self.current.is_none() {
            return;
        }

        if line.starts_with("@@") {
            self.open_hunk(line);
            return;
        }

        self.handle_file_metadata(line);
    }

    fn open_hunk(&mut self, line: &str) {
        if let Some(parsed) = parse_hunk_header(line) {
            self.open_parsed_hunk(parsed);
        }
    }

    fn reopen_hunk_at(&mut self, line: &str) -> bool {
        let Some(parsed) = line
            .starts_with("@@")
            .then(|| parse_hunk_header(line))
            .flatten()
        else {
            return false;
        };
        self.flush_hunk();
        self.open_parsed_hunk(parsed);
        true
    }

    fn open_parsed_hunk(&mut self, (header, header_raw): (HunkHeader, String)) {
        let expected_lines = (header.old_count as usize)
            .saturating_add(header.new_count as usize)
            .min(MAX_HUNK_PREALLOC_LINES);
        self.hunk_old_left = header.old_count;
        self.hunk_new_left = header.new_count;
        self.current_hunk = Some(Hunk {
            header,
            header_raw,
            lines: Vec::with_capacity(expected_lines),
        });
    }

    fn push_hunk_line(&mut self, line: &str) -> bool {
        if self.word_diff != WordDiffFormat::None {
            return self.push_word_diff_line(line);
        }
        let Some(hunk) = self.current_hunk.as_mut() else {
            return false;
        };
        let parent_count = hunk.header.parent_count;
        if parent_count >= 2 {
            if let Some(hunk_line) = classify_hunk_line(line, parent_count) {
                hunk.lines.push(hunk_line);
                return true;
            }
            self.flush_hunk();
            return false;
        }

        if self.hunk_old_left == 0 && self.hunk_new_left == 0 {
            if line.starts_with('\\') {
                hunk.lines.push(HunkLine::NoNewline);
                return true;
            }
            self.flush_hunk();
            return false;
        }

        if self.reopen_hunk_at(line) {
            return true;
        }

        let hunk_line = classify_counted_hunk_line(line);
        let (uses_old, uses_new) = hunk_line.sides();
        if uses_old {
            self.hunk_old_left = self.hunk_old_left.saturating_sub(1);
        }
        if uses_new {
            self.hunk_new_left = self.hunk_new_left.saturating_sub(1);
        }
        if let Some(hunk) = self.current_hunk.as_mut() {
            hunk.lines.push(hunk_line);
        }
        true
    }

    fn push_word_diff_line(&mut self, line: &str) -> bool {
        if self.reopen_hunk_at(line) {
            return true;
        }
        let porcelain = self.word_diff == WordDiffFormat::Porcelain;
        let Some(hunk) = self.current_hunk.as_mut() else {
            return false;
        };
        if line.starts_with("\\ ") {
            hunk.lines.push(HunkLine::NoNewline);
            return true;
        }
        if porcelain {
            let segment = |kind: WordDiffKind, text: &str| WordDiffSegment {
                kind,
                text: text.to_string(),
            };
            match line.chars().next() {
                Some('~') => {
                    let segments = std::mem::take(&mut self.porcelain_segments);
                    hunk.lines.push(word_diff_hunk_line(segments));
                }
                Some(first) => {
                    let kind = match first {
                        '+' => WordDiffKind::Add,
                        '-' => WordDiffKind::Del,
                        ' ' => WordDiffKind::Common,
                        _ => return false,
                    };
                    self.porcelain_segments.push(segment(kind, &line[1..]));
                }
                None => return false,
            }
            return true;
        }
        hunk.lines.push(bare_word_diff_line(line));
        true
    }

    fn try_plain_file_header(&mut self, line: &str) -> bool {
        if let Some(old) = self.pending_plain_old.take() {
            if let Some(rest) = line.strip_prefix("+++ ") {
                self.flush_file();
                let new = plain_header_path(rest);
                let header = format!("--- {old}");
                let mut file = DiffFile::new(String::new(), String::new(), &header);
                file.header_lines.push(line.to_string());
                if let Some(command) = self.pending_plain_header.take() {
                    file.header_lines.insert(0, command);
                }
                let old_path = plain_header_path(&old);
                set_header_old_path(&mut file, old_path);
                set_header_new_path(&mut file, new);
                self.current = Some(file);
                return true;
            }
        }
        let starts_plain_file = self.current.as_ref().is_none_or(|f| !f.hunks.is_empty());
        if starts_plain_file && self.current_hunk.is_none() {
            if let Some(rest) = line.strip_prefix("--- ") {
                self.pending_plain_old = Some(rest.to_string());
                return true;
            }
        }
        false
    }

    fn open_submodule(&mut self, line: &str) {
        self.flush_hunk();
        let path = submodule_marker_path(line);
        let same_file = self.current.as_ref().is_some_and(|f| f.new_path == path);
        if same_file {
            if let Some(current) = self.current.as_mut() {
                current.header_lines.push(line.to_string());
            }
        } else {
            self.flush_file();
            self.current = Some(DiffFile::new(path.clone(), path.clone(), line));
        }
        if let Some(current) = self.current.as_mut() {
            current.change = FileChange::Submodule;
        }
        self.submodule_prefix = format!("{path}/");
        self.submodule_prefixed = None;
        self.submodule_nested_buf = Some(String::new());
    }

    fn flush_submodule_nested(&mut self) {
        if let Some(buf) = self.submodule_nested_buf.take() {
            let nested = parse_unified_diff(&buf);
            if let Some(current) = self.current.as_mut() {
                if !nested.is_empty() {
                    current.nested_submodule_diff = Some(nested);
                }
            }
        }
    }

    fn handle_file_metadata(&mut self, line: &str) {
        let Some(current) = self.current.as_mut() else {
            return;
        };
        current.header_lines.push(line.to_string());

        if let Some(rest) = line.strip_prefix("--- ") {
            set_header_old_path(current, decode_header_path(rest));
            return;
        }
        if let Some(rest) = line.strip_prefix("+++ ") {
            set_header_new_path(current, decode_header_path(rest));
            return;
        }
        if let Some(rest) = line.strip_prefix("similarity index ") {
            let similarity = rest.trim_end_matches('%').trim().parse::<u16>().ok();
            promote_to_rename(current, |change| {
                if let FileChange::Renamed { similarity: s, .. } = change {
                    *s = similarity;
                }
            });
            return;
        }
        if let Some(rest) = line.strip_prefix("rename from ") {
            let from = decode_path(rest.trim());
            promote_to_rename(current, |change| {
                if let FileChange::Renamed { from: f, .. } = change {
                    *f = from.clone();
                }
            });

            current.old_path = from;
            return;
        }
        if let Some(rest) = line.strip_prefix("rename to ") {
            let to = decode_path(rest.trim());
            promote_to_rename(current, |change| {
                if let FileChange::Renamed { to: t, .. } = change {
                    *t = to.clone();
                }
            });
            current.new_path = to;
            return;
        }
        if let Some(rest) = line.strip_prefix("old mode ") {
            let old_mode = rest.trim().to_string();
            if let FileChange::ModeOnly {
                old_mode: existing, ..
            } = &mut current.change
            {
                *existing = old_mode;
            } else if matches!(current.change, FileChange::Modified) {
                current.change = FileChange::ModeOnly {
                    old_mode,
                    new_mode: String::new(),
                };
            }
            return;
        }
        if let Some(rest) = line.strip_prefix("new mode ") {
            let new_mode = rest.trim().to_string();
            if let FileChange::ModeOnly {
                new_mode: existing, ..
            } = &mut current.change
            {
                *existing = new_mode;
            } else if matches!(current.change, FileChange::Modified) {
                current.change = FileChange::ModeOnly {
                    old_mode: String::new(),
                    new_mode,
                };
            }
            return;
        }
        if let Some(rest) = line.strip_prefix("Binary files ") {
            if let Some(paths) = rest.strip_suffix(" differ") {
                apply_binary_paths(current, paths);
                mark_binary(current);
                return;
            }
        }
        if line.starts_with("GIT binary patch") {
            mark_binary(current);
            return;
        }
        if line.starts_with("Subproject commit ") {
            current.change = FileChange::Submodule;
            return;
        }
        if let Some(rest) = line.strip_prefix("index ") {
            let mut parts = rest.split_whitespace();
            let payload = parts.next().unwrap_or("");
            let mode = parts.next().unwrap_or("");
            if let Some((old_sha, new_sha)) = payload.split_once("..") {
                if !old_sha.is_empty() {
                    current.old_blob_sha = Some(old_sha.to_string());
                }
                if !new_sha.is_empty() {
                    current.new_blob_sha = Some(new_sha.to_string());
                }
            }
            if mode == GITLINK_MODE && matches!(current.change, FileChange::Modified) {
                current.change = FileChange::Submodule;
            }
            return;
        }

        if line.starts_with("new file mode ") && matches!(current.change, FileChange::Modified) {
            current.change = FileChange::Added;
        }
        if line.starts_with("deleted file mode ") && matches!(current.change, FileChange::Modified)
        {
            current.change = FileChange::Deleted;
        }
    }

    fn flush_hunk(&mut self) {
        if !self.porcelain_segments.is_empty() {
            let segments = std::mem::take(&mut self.porcelain_segments);
            if let Some(hunk) = self.current_hunk.as_mut() {
                hunk.lines.push(word_diff_hunk_line(segments));
            }
        }
        self.hunk_old_left = 0;
        self.hunk_new_left = 0;
        if let Some(hunk) = self.current_hunk.take() {
            if let Some(current) = self.current.as_mut() {
                current.hunks.push(hunk);
            }
        }
    }

    fn flush_file(&mut self) {
        self.flush_submodule_nested();
        if let Some(file) = self.current.take() {
            self.files.push(file);
        }
    }

    fn finish(mut self) -> Vec<DiffFile> {
        self.flush_hunk();
        self.flush_file();
        self.files
    }
}

const DEV_NULL: &str = "/dev/null";

fn set_header_old_path(file: &mut DiffFile, path: String) {
    if path == DEV_NULL {
        file.change = FileChange::Added;
        file.old_path = String::new();
        return;
    }
    file.old_path = strip_prefix(&path).to_string();
}

fn set_header_new_path(file: &mut DiffFile, path: String) {
    if path == DEV_NULL {
        file.change = FileChange::Deleted;
        file.new_path = String::new();
        return;
    }
    file.new_path = strip_prefix(&path).to_string();
}

fn mark_binary(file: &mut DiffFile) {
    file.binary = true;
    if matches!(file.change, FileChange::Modified) {
        file.change = FileChange::Binary;
    }
}

fn apply_binary_paths(file: &mut DiffFile, paths: &str) {
    if paths.starts_with('"') {
        if let Some((a, tail)) = take_quoted(paths) {
            let b = tail
                .strip_prefix(" and ")
                .map(decode_path)
                .unwrap_or_default();
            set_binary_side_paths(file, a, b);
        }
        return;
    }
    let candidates: Vec<usize> = paths.match_indices(" and ").map(|(i, _)| i).collect();
    let agrees = |i: usize| {
        let (a, b) = (&paths[..i], &paths[i + 5..]);
        a == DEV_NULL
            || b == DEV_NULL
            || (!file.old_path.is_empty() && strip_prefix(a) == file.old_path)
            || (!file.new_path.is_empty() && strip_prefix(b) == file.new_path)
    };
    let unknown = file.old_path.is_empty() && file.new_path.is_empty();
    let split = candidates.iter().copied().find(|&i| agrees(i)).or_else(|| {
        unknown
            .then(|| {
                candidates
                    .iter()
                    .copied()
                    .find(|&i| strip_prefix(&paths[..i]) == strip_prefix(&paths[i + 5..]))
                    .or_else(|| candidates.first().copied())
            })
            .flatten()
    });
    if let Some(i) = split {
        set_binary_side_paths(file, paths[..i].to_string(), decode_path(&paths[i + 5..]));
    }
}

fn set_binary_side_paths(file: &mut DiffFile, a: String, b: String) {
    if !a.is_empty() {
        set_header_old_path(file, a);
    }
    if !b.is_empty() {
        set_header_new_path(file, b);
    }
}

fn is_plain_diff_command(line: &str) -> bool {
    let Some(rest) = line.strip_prefix("diff ") else {
        return false;
    };
    let mut has_option = false;
    let mut has_operand = false;
    for token in rest.split_whitespace() {
        if token.starts_with('-') {
            has_option = true;
        } else {
            has_operand = true;
        }
    }
    has_option && has_operand
}

fn plain_header_path(rest: &str) -> String {
    match rest.split_once('\t') {
        Some((_, stamp)) if is_epoch_stamp(stamp) => DEV_NULL.to_string(),
        _ => decode_header_path(rest),
    }
}

fn is_epoch_stamp(stamp: &str) -> bool {
    let mut parts = stamp.split_whitespace();
    let (Some(date), Some(time)) = (parts.next(), parts.next()) else {
        return false;
    };
    let numbers = |text: &str, sep: char| -> Option<[i64; 3]> {
        let parsed: Option<Vec<i64>> = text.split(sep).map(|n| n.parse::<i64>().ok()).collect();
        parsed.and_then(|v| v.try_into().ok())
    };
    let Some([y, m, d]) = numbers(date, '-') else {
        return false;
    };
    let whole_seconds = time.split('.').next().unwrap_or(time);
    let Some([h, min, sec]) = numbers(whole_seconds, ':') else {
        return false;
    };
    let in_range = (1969..=1970).contains(&y)
        && (1..=12).contains(&m)
        && (1..=31).contains(&d)
        && (0..=23).contains(&h)
        && (0..=59).contains(&min)
        && (0..=60).contains(&sec);
    if !in_range {
        return false;
    }
    let offset_minutes = parts.next().map_or(0, parse_utc_offset_minutes);
    let local = days_from_civil(y, m, d) * 86_400 + h * 3_600 + min * 60 + sec;
    local - offset_minutes * 60 == 0
}

fn parse_utc_offset_minutes(offset: &str) -> i64 {
    let bytes = offset.as_bytes();
    let sign = match bytes.first() {
        Some(b'+') => 1,
        Some(b'-') => -1,
        _ => return 0,
    };
    if bytes.len() != 5 || !bytes[1..].iter().all(u8::is_ascii_digit) {
        return 0;
    }
    let hours: i64 = offset[1..3].parse().unwrap_or(0);
    let minutes: i64 = offset[3..5].parse().unwrap_or(0);
    sign * (hours * 60 + minutes)
}

fn days_from_civil(y: i64, m: i64, d: i64) -> i64 {
    let y = if m <= 2 { y - 1 } else { y };
    let era = y.div_euclid(400);
    let yoe = y - era * 400;
    let mp = (m + 9) % 12;
    let doy = (153 * mp + 2) / 5 + d - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146_097 + doe - 719_468
}

fn word_diff_hunk_line(segments: Vec<WordDiffSegment>) -> HunkLine {
    if segments.iter().all(|s| s.kind == WordDiffKind::Common) {
        HunkLine::Context(segments.into_iter().map(|s| s.text).collect())
    } else {
        HunkLine::WordDiff(segments)
    }
}

fn decode_header_path(rest: &str) -> String {
    let rest = rest.trim_end();
    if rest.starts_with('"') {
        return decode_path(rest);
    }
    rest.split('\t').next().unwrap_or(rest).to_string()
}

fn decode_path(raw: &str) -> String {
    let raw = raw.trim_end();
    match take_quoted(raw) {
        Some((path, _)) => path,
        None => raw.to_string(),
    }
}

fn take_quoted(s: &str) -> Option<(String, &str)> {
    let bytes = s.as_bytes();
    if bytes.first() != Some(&b'"') {
        return None;
    }
    let mut out: Vec<u8> = Vec::with_capacity(bytes.len());
    let mut i = 1;
    while i < bytes.len() {
        match bytes[i] {
            b'"' => return Some((String::from_utf8_lossy(&out).into_owned(), &s[i + 1..])),
            b'\\' if i + 1 < bytes.len() => {
                let c = bytes[i + 1];
                let octal = |b: u8| (b'0'..=b'7').contains(&b);
                if octal(c) && i + 3 < bytes.len() && octal(bytes[i + 2]) && octal(bytes[i + 3]) {
                    let value = (c - b'0') as u32 * 64
                        + (bytes[i + 2] - b'0') as u32 * 8
                        + (bytes[i + 3] - b'0') as u32;
                    out.push(value as u8);
                    i += 4;
                    continue;
                }
                out.push(match c {
                    b'a' => 0x07,
                    b'b' => 0x08,
                    b't' => b'\t',
                    b'n' => b'\n',
                    b'v' => 0x0b,
                    b'f' => 0x0c,
                    b'r' => b'\r',
                    other => other,
                });
                i += 2;
            }
            b => {
                out.push(b);
                i += 1;
            }
        }
    }
    None
}

fn submodule_marker_path(line: &str) -> String {
    let rest = line.strip_prefix("Submodule ").unwrap_or(line);
    rest.rsplit_once(' ')
        .map_or(rest, |(path, _)| path)
        .to_string()
}

fn is_submodule_marker_line(line: &str) -> bool {
    if let Some(rest) = line.strip_prefix("Submodule ") {
        return rest.contains("..") && rest.trim_end().ends_with(':');
    }
    false
}

fn classify_hunk_line(line: &str, parent_count: u8) -> Option<HunkLine> {
    if line == super::NO_NEWLINE_MARKER {
        return Some(HunkLine::NoNewline);
    }

    if parent_count >= 2 {
        let n = parent_count as usize;
        if line.is_empty() {
            return Some(HunkLine::Context(String::new()));
        }

        let mut chars = line.chars();
        let mut prefix = String::with_capacity(n);
        for _ in 0..n {
            match chars.next() {
                Some(c) if c == '+' || c == '-' || c == ' ' => prefix.push(c),
                Some('\\') if prefix.is_empty() => return Some(HunkLine::NoNewline),
                _ => return None,
            }
        }
        let content: String = chars.as_str().to_string();

        if prefix.chars().all(|c| c == ' ') {
            return Some(HunkLine::Context(content));
        }
        return Some(HunkLine::Combined(prefix, content));
    }
    let mut chars = line.chars();
    match chars.next() {
        Some('+') => Some(HunkLine::Add(chars.as_str().to_string())),
        Some('-') => Some(HunkLine::Del(chars.as_str().to_string())),
        Some(' ') => Some(HunkLine::Context(chars.as_str().to_string())),
        Some('\\') => Some(HunkLine::NoNewline),

        None => Some(HunkLine::Context(String::new())),
        _ => {
            if let Some(segments) = try_parse_word_diff(line) {
                return Some(HunkLine::WordDiff(segments));
            }
            None
        }
    }
}

fn classify_counted_hunk_line(line: &str) -> HunkLine {
    let mut chars = line.chars();
    match chars.next() {
        Some('+') => HunkLine::Add(chars.as_str().to_string()),
        Some('-') => HunkLine::Del(chars.as_str().to_string()),
        Some(' ') => HunkLine::Context(chars.as_str().to_string()),
        Some('\\') => HunkLine::NoNewline,
        None => HunkLine::Context(String::new()),
        Some(_) => bare_word_diff_line(line),
    }
}

fn bare_word_diff_line(line: &str) -> HunkLine {
    match try_parse_word_diff(line) {
        Some(segments) => HunkLine::WordDiff(segments),
        None => HunkLine::Context(line.to_string()),
    }
}

fn try_parse_word_diff(line: &str) -> Option<Vec<WordDiffSegment>> {
    let has_del = line.contains("[-");
    let has_add = line.contains("{+");
    if !has_del && !has_add {
        return None;
    }
    let bytes = line.as_bytes();
    let mut segments: Vec<WordDiffSegment> = Vec::new();
    let mut common_start = 0usize;
    let mut i = 0usize;
    while i < bytes.len() {
        if i + 1 < bytes.len() && bytes[i] == b'[' && bytes[i + 1] == b'-' {
            if let Some(end) = find_marker_end(bytes, i + 2, b'-', b']') {
                if i > common_start {
                    segments.push(WordDiffSegment {
                        kind: WordDiffKind::Common,
                        text: line[common_start..i].to_string(),
                    });
                }
                segments.push(WordDiffSegment {
                    kind: WordDiffKind::Del,
                    text: line[i + 2..end].to_string(),
                });
                i = end + 2;
                common_start = i;
                continue;
            }
        }
        if i + 1 < bytes.len() && bytes[i] == b'{' && bytes[i + 1] == b'+' {
            if let Some(end) = find_marker_end(bytes, i + 2, b'+', b'}') {
                if i > common_start {
                    segments.push(WordDiffSegment {
                        kind: WordDiffKind::Common,
                        text: line[common_start..i].to_string(),
                    });
                }
                segments.push(WordDiffSegment {
                    kind: WordDiffKind::Add,
                    text: line[i + 2..end].to_string(),
                });
                i = end + 2;
                common_start = i;
                continue;
            }
        }
        i += 1;
    }
    if segments.is_empty() {
        return None;
    }
    if common_start < bytes.len() {
        segments.push(WordDiffSegment {
            kind: WordDiffKind::Common,
            text: line[common_start..].to_string(),
        });
    }
    Some(segments)
}

fn find_marker_end(bytes: &[u8], from: usize, close1: u8, close2: u8) -> Option<usize> {
    let mut j = from;
    while j + 1 < bytes.len() {
        if bytes[j] == close1 && bytes[j + 1] == close2 {
            return Some(j);
        }
        j += 1;
    }
    None
}

fn parse_hunk_header(line: &str) -> Option<(HunkHeader, String)> {
    let at_count = line.chars().take_while(|&c| c == '@').count();
    if at_count < 2 {
        return None;
    }
    let inner = &line[at_count..];

    let close = "@".repeat(at_count);
    let close_idx = inner.find(close.as_str())?;
    let range_part = &inner[..close_idx];
    let tail = &inner[close_idx + at_count..];
    let range = range_part.trim();

    let mut old_last: Option<(u32, u32)> = None;
    let mut minus_count: u8 = 0;
    let mut new_range: Option<(u32, u32)> = None;
    for tok in range.split_whitespace() {
        if let Some(rest) = tok.strip_prefix('-') {
            old_last = Some(parse_range_pair(rest));
            minus_count = minus_count.saturating_add(1);
        } else if let Some(rest) = tok.strip_prefix('+') {
            new_range = Some(parse_range_pair(rest));
        }
    }
    let (old_start, old_count) = old_last?;
    let (new_start, new_count) = new_range?;
    let section = {
        let trimmed = tail.trim();
        if trimmed.is_empty() {
            None
        } else {
            Some(trimmed.to_string())
        }
    };

    let parent_count = if minus_count >= 2 {
        minus_count
    } else if at_count >= 3 {
        (at_count - 1) as u8
    } else {
        1
    };
    Some((
        HunkHeader {
            old_start,
            old_count,
            new_start,
            new_count,
            section,
            parent_count,
        },
        line.to_string(),
    ))
}

fn parse_range_pair(s: &str) -> (u32, u32) {
    let (a, b) = match s.split_once(',') {
        Some((a, b)) => (a, Some(b)),
        None => (s, None),
    };
    let start = a.parse::<u32>().unwrap_or(0);
    let count = match b {
        Some(b) => b.parse::<u32>().unwrap_or(1),
        None => 1,
    };
    (start, count)
}

fn split_diff_git_paths(rest: &str) -> (String, String) {
    if let Some((a, tail)) = take_quoted(rest) {
        let b = decode_path(tail.trim_start());
        return (strip_prefix(&a).to_string(), strip_prefix(&b).to_string());
    }
    if let Some(idx) = rest.find(" \"") {
        let b = decode_path(&rest[idx + 1..]);
        return (
            strip_prefix(&rest[..idx]).to_string(),
            strip_prefix(&b).to_string(),
        );
    }
    if let Some(path) = symmetric_git_path(rest) {
        return (path.to_string(), path.to_string());
    }
    if let Some(idx) = rest.find(" b/") {
        return (
            strip_prefix(&rest[..idx]).to_string(),
            strip_prefix(&rest[idx + 1..]).to_string(),
        );
    }

    let tokens: Vec<&str> = rest.split_whitespace().collect();
    if tokens.len() >= 2 {
        let a = tokens[tokens.len() - 2];
        let b = tokens[tokens.len() - 1];
        return (strip_prefix(a).to_string(), strip_prefix(b).to_string());
    }
    (String::new(), String::new())
}

fn symmetric_git_path(rest: &str) -> Option<&str> {
    let body = rest.strip_prefix("a/")?;
    let len = body.len().checked_sub(3)?;
    if len % 2 != 0 || !body.is_char_boundary(len / 2) {
        return None;
    }
    let (path, tail) = body.split_at(len / 2);
    (tail.strip_prefix(" b/")? == path).then_some(path)
}

fn promote_to_rename(file: &mut DiffFile, mutator: impl FnOnce(&mut FileChange)) {
    if !matches!(file.change, FileChange::Renamed { .. }) {
        let from = file.old_path.clone();
        let to = file.new_path.clone();
        file.change = FileChange::Renamed {
            from,
            to,
            similarity: None,
        };
    }
    mutator(&mut file.change);
}
