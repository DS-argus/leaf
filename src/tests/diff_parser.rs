use crate::diff::{parse_unified_diff, FileChange, HunkLine, WordDiffKind};

fn fixture(name: &str) -> String {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/diff")
        .join(name);
    std::fs::read_to_string(&path)
        .unwrap_or_else(|e| panic!("read fixture {}: {e}", path.display()))
}

#[test]
fn basic_single_file_single_hunk() {
    let diff = fixture("basic.diff");
    let files = parse_unified_diff(&diff);
    assert_eq!(files.len(), 1);
    let f = &files[0];
    assert_eq!(f.old_path, "src/foo.rs");
    assert_eq!(f.new_path, "src/foo.rs");
    assert!(matches!(f.change, FileChange::Modified));
    assert_eq!(f.hunks.len(), 1);
    let h = &f.hunks[0];
    assert_eq!(h.header.old_start, 1);
    assert_eq!(h.header.old_count, 3);
    assert_eq!(h.header.new_start, 1);
    assert_eq!(h.header.new_count, 3);
    assert_eq!(h.lines.len(), 4);
    assert!(matches!(h.lines[0], HunkLine::Context(ref s) if s == "fn main() {"));
    assert!(matches!(h.lines[1], HunkLine::Del(ref s) if s == "    println!(\"hello\");"));
    assert!(matches!(h.lines[2], HunkLine::Add(ref s) if s == "    println!(\"world\");"));
    assert!(matches!(h.lines[3], HunkLine::Context(ref s) if s == "}"));
}

#[test]
fn create_dev_null_marks_added() {
    let files = parse_unified_diff(&fixture("create.diff"));
    assert_eq!(files.len(), 1);
    let f = &files[0];
    assert!(
        matches!(f.change, FileChange::Added),
        "expected Added, got {:?}",
        f.change
    );
    assert_eq!(f.new_path, "src/new.rs");
    assert_eq!(f.old_path, "");
    assert_eq!(f.hunks.len(), 1);
    assert!(f
        .hunks
        .iter()
        .flat_map(|h| h.lines.iter())
        .any(|l| matches!(l, HunkLine::Add(_))));
}

#[test]
fn delete_dev_null_marks_deleted() {
    let files = parse_unified_diff(&fixture("delete.diff"));
    assert_eq!(files.len(), 1);
    assert!(
        matches!(files[0].change, FileChange::Deleted),
        "got {:?}",
        files[0].change
    );
    assert_eq!(files[0].new_path, "");
    assert_eq!(files[0].old_path, "src/old.rs");
}

#[test]
fn rename_with_similarity_index() {
    let files = parse_unified_diff(&fixture("rename.diff"));
    assert_eq!(files.len(), 1);
    let f = &files[0];
    match &f.change {
        FileChange::Renamed {
            from,
            to,
            similarity,
        } => {
            assert_eq!(from, "src/old_name.rs");
            assert_eq!(to, "src/new_name.rs");
            assert_eq!(*similarity, Some(92));
        }
        other => panic!("expected Renamed, got {other:?}"),
    }
    assert_eq!(f.hunks.len(), 1);

    assert_eq!(f.hunks[0].header.section.as_deref(), Some("fn helper() {"));
}

#[test]
fn mode_change_without_hunk() {
    let files = parse_unified_diff(&fixture("mode_change.diff"));
    assert_eq!(files.len(), 1);
    match &files[0].change {
        FileChange::ModeOnly { old_mode, new_mode } => {
            assert_eq!(old_mode, "100644");
            assert_eq!(new_mode, "100755");
        }
        other => panic!("expected ModeOnly, got {other:?}"),
    }
    assert!(files[0].hunks.is_empty());
}

#[test]
fn binary_files_differ_marker() {
    let files = parse_unified_diff(&fixture("binary.diff"));
    assert_eq!(files.len(), 1);
    assert!(matches!(files[0].change, FileChange::Binary));
    assert!(files[0].hunks.is_empty());
}

#[test]
fn git_binary_patch_marker() {
    let files = parse_unified_diff(&fixture("binary_patch.diff"));
    assert_eq!(files.len(), 1);
    assert!(matches!(files[0].change, FileChange::Binary));
    assert!(files[0].hunks.is_empty());
}

#[test]
fn submodule_change_recognized_via_content() {
    let files = parse_unified_diff(&fixture("submodule.diff"));
    assert_eq!(files.len(), 1);

    assert!(matches!(files[0].change, FileChange::Submodule));
    let has_subproject = files[0].hunks.iter().flat_map(|h| h.lines.iter()).any(
        |l| matches!(l, HunkLine::Add(s) | HunkLine::Del(s) if s.starts_with("Subproject commit")),
    );
    assert!(has_subproject);
}

#[test]
fn no_newline_marker_captured() {
    let files = parse_unified_diff(&fixture("no_newline.diff"));
    assert_eq!(files.len(), 1);
    let markers = files[0]
        .hunks
        .iter()
        .flat_map(|h| h.lines.iter())
        .filter(|l| matches!(l, HunkLine::NoNewline))
        .count();
    assert_eq!(markers, 2);
}

#[test]
fn no_prefix_paths_are_captured_verbatim() {
    let files = parse_unified_diff(&fixture("no_prefix.diff"));
    assert_eq!(files.len(), 1);
    assert_eq!(files[0].old_path, "src/foo.rs");
    assert_eq!(files[0].new_path, "src/foo.rs");
    assert_eq!(files[0].hunks.len(), 1);
}

#[test]
fn multi_file_yields_multiple_diff_files() {
    let files = parse_unified_diff(&fixture("multi_file.diff"));
    assert_eq!(files.len(), 2);
    assert_eq!(files[0].new_path, "first.rs");
    assert_eq!(files[1].new_path, "second.rs");
}

#[test]
fn empty_input_yields_empty_result() {
    assert!(parse_unified_diff("").is_empty());
}

#[test]
fn unknown_leading_noise_is_ignored() {
    let src = "some random preamble line\nnot a diff\n";
    assert!(parse_unified_diff(src).is_empty());
}

#[test]
fn hunk_short_form_counts_default_to_one() {
    let src = "diff --git a/x b/x\n--- a/x\n+++ b/x\n@@ -1 +1 @@\n-a\n+b\n";
    let files = parse_unified_diff(src);
    let h = &files[0].hunks[0];
    assert_eq!(h.header.old_count, 1);
    assert_eq!(h.header.new_count, 1);
}

#[test]
fn diff_cc_starts_a_file_entry() {
    let src = "diff --cc merged.rs\n@@@ -1,2 -1,2 +1,2 @@@\n foo\n";
    let files = parse_unified_diff(src);
    assert_eq!(files.len(), 1);

    assert!(files[0].old_path.contains("merged.rs"));
}

#[test]
fn very_large_hunk_parses_without_panic() {
    let mut src = String::from(
        "diff --git a/big.rs b/big.rs\n--- a/big.rs\n+++ b/big.rs\n@@ -1,1000 +1,1000 @@\n",
    );
    for i in 0..1000 {
        if i % 2 == 0 {
            src.push_str(&format!(" line {i}\n"));
        } else {
            src.push_str(&format!("-line {i}\n+LINE {i}\n"));
        }
    }
    let files = parse_unified_diff(&src);
    assert_eq!(files.len(), 1);
    assert_eq!(files[0].hunks.len(), 1);
    assert!(files[0].hunks[0].lines.len() >= 1000);
}

#[test]
fn combined_diff_recognizes_at_at_at_hunk_header() {
    let files = parse_unified_diff(&fixture("combined.diff"));
    assert_eq!(files.len(), 1);
    let f = &files[0];
    assert!(f.old_path.contains("src/merge.rs"));
    assert_eq!(f.hunks.len(), 1);
    let h = &f.hunks[0];
    assert_eq!(h.header.parent_count, 2, "expected 2-parent combined hunk");
    assert!(h
        .lines
        .iter()
        .any(|l| matches!(l, HunkLine::Combined(_, _))));
}

#[test]
fn combined_diff_uniform_context_stays_as_context() {
    let files = parse_unified_diff(&fixture("combined.diff"));
    let h = &files[0].hunks[0];

    let context_count = h
        .lines
        .iter()
        .filter(|l| matches!(l, HunkLine::Context(_)))
        .count();
    assert!(
        context_count >= 1,
        "expected at least one pure-context line, got hunk lines: {:?}",
        h.lines
    );
}

#[test]
fn word_diff_line_splits_into_segments() {
    let files = parse_unified_diff(&fixture("word_diff.diff"));
    assert_eq!(files.len(), 1);
    let f = &files[0];
    assert_eq!(f.hunks.len(), 1);
    let h = &f.hunks[0];
    let word_diff_lines: Vec<_> = h
        .lines
        .iter()
        .filter_map(|l| match l {
            HunkLine::WordDiff(segs) => Some(segs.clone()),
            _ => None,
        })
        .collect();
    assert_eq!(word_diff_lines.len(), 2);
    let first = &word_diff_lines[0];

    assert!(matches!(first[0].kind, WordDiffKind::Common));
    assert!(first
        .iter()
        .any(|s| matches!(s.kind, WordDiffKind::Del) && s.text == "a"));
    assert!(first
        .iter()
        .any(|s| matches!(s.kind, WordDiffKind::Add) && s.text == "the"));
}

#[test]
fn submodule_recursive_nested_diff_is_attached() {
    let files = parse_unified_diff(&fixture("submodule_recursive.diff"));
    assert_eq!(files.len(), 1);
    let f = &files[0];
    assert!(matches!(f.change, FileChange::Submodule));
    let nested = f
        .nested_submodule_diff
        .as_ref()
        .expect("nested submodule diff attached");
    assert_eq!(nested.len(), 1);
    assert_eq!(nested[0].new_path, "nested.rs");
    assert_eq!(nested[0].hunks.len(), 1);
}

#[test]
fn index_line_captures_blob_shas() {
    let src = "diff --git a/x b/x\n\
               index abc1234..def5678 100644\n\
               --- a/x\n\
               +++ b/x\n\
               @@ -1,1 +1,1 @@\n\
               -a\n\
               +b\n";
    let files = parse_unified_diff(src);
    assert_eq!(files.len(), 1);
    let f = &files[0];
    assert_eq!(f.old_blob_sha.as_deref(), Some("abc1234"));
    assert_eq!(f.new_blob_sha.as_deref(), Some("def5678"));
}

#[test]
fn index_line_without_mode_still_captures_shas() {
    let src = "diff --git a/y b/y\n\
               index 0123456789abcdef..fedcba9876543210\n\
               --- a/y\n\
               +++ b/y\n\
               @@ -1,1 +1,1 @@\n\
               -a\n\
               +b\n";
    let files = parse_unified_diff(src);
    assert_eq!(files.len(), 1);
    assert_eq!(files[0].old_blob_sha.as_deref(), Some("0123456789abcdef"));
    assert_eq!(files[0].new_blob_sha.as_deref(), Some("fedcba9876543210"));
}

#[test]
fn missing_index_line_leaves_blob_shas_none() {
    let src = "diff --git a/z b/z\n--- a/z\n+++ b/z\n@@ -1 +1 @@\n-a\n+b\n";
    let files = parse_unified_diff(src);
    assert_eq!(files.len(), 1);
    assert!(files[0].old_blob_sha.is_none());
    assert!(files[0].new_blob_sha.is_none());
}

#[test]
fn lossy_input_does_not_panic() {
    let src = "diff --git a/x b/x\n--- a/x\n+++ b/x\n@@ -1,1 +1,1 @@\n-\u{FFFD}old\n+\u{FFFD}new\n";
    let files = parse_unified_diff(src);
    assert_eq!(files.len(), 1);
    assert_eq!(files[0].hunks.len(), 1);
}

#[test]
fn huge_hunk_counts_do_not_overflow() {
    let src = "diff --git a/x b/x\n--- a/x\n+++ b/x\n\
               @@ -1,4294967295 +1,4294967295 @@\n-a\n+b\n";
    let files = parse_unified_diff(src);
    assert_eq!(files.len(), 1);
    assert_eq!(files[0].hunks[0].lines.len(), 2);
}

const SUBMODULE_TAIL: &str = "diff --git a/top.txt b/top.txt\nindex 718f4d2..4ae8ef0 100644\n\
                              --- a/top.txt\n+++ b/top.txt\n@@ -1 +1 @@\n-t\n+u\n";

#[test]
fn submodule_diff_output_keeps_following_files() {
    let src = format!(
        "diff --git a/a.txt b/a.txt\nindex 7898192..6178079 100644\n--- a/a.txt\n+++ b/a.txt\n\
         @@ -1 +1 @@\n-a\n+b\nSubmodule sub 4cccf3d..892e5ad:\n\
         diff --git a/sub/f.txt b/sub/f.txt\nindex 5626abf..f719efd 100644\n--- a/sub/f.txt\n\
         +++ b/sub/f.txt\n@@ -1 +1 @@\n-one\n+two\n{SUBMODULE_TAIL}"
    );
    let files = parse_unified_diff(&src);
    let paths: Vec<&str> = files.iter().map(|f| f.new_path.as_str()).collect();
    assert_eq!(paths, ["a.txt", "sub", "top.txt"]);
    assert!(matches!(files[0].change, FileChange::Modified));
    assert!(matches!(files[1].change, FileChange::Submodule));
    let nested = files[1].nested_submodule_diff.as_ref().unwrap();
    assert_eq!(nested[0].new_path, "sub/f.txt");
}

#[test]
fn submodule_log_output_keeps_following_files() {
    let src = format!(
        "diff --git a/a.txt b/a.txt\n--- a/a.txt\n+++ b/a.txt\n@@ -1 +1 @@\n-a\n+b\n\
         Submodule sub 4cccf3d..892e5ad:\n  > c\n{SUBMODULE_TAIL}"
    );
    let files = parse_unified_diff(&src);
    let paths: Vec<&str> = files.iter().map(|f| f.new_path.as_str()).collect();
    assert_eq!(paths, ["a.txt", "sub", "top.txt"]);
}

#[test]
fn consecutive_submodules_stay_separate() {
    let log = format!(
        "Submodule subA 1111111..2222222:\n  > a\nSubmodule subB 3333333..4444444:\n  > b\n{SUBMODULE_TAIL}"
    );
    let files = parse_unified_diff(&log);
    let paths: Vec<&str> = files.iter().map(|f| f.new_path.as_str()).collect();
    assert_eq!(paths, ["subA", "subB", "top.txt"]);

    let diff = format!(
        "Submodule subA 1111111..2222222:\ndiff --git a/subA/x b/subA/x\n--- a/subA/x\n+++ b/subA/x\n\
         @@ -1 +1 @@\n-1\n+2\nSubmodule subB 3333333..4444444:\ndiff --git a/subB/y b/subB/y\n\
         --- a/subB/y\n+++ b/subB/y\n@@ -1 +1 @@\n-1\n+2\n{SUBMODULE_TAIL}"
    );
    let files = parse_unified_diff(&diff);
    let paths: Vec<&str> = files.iter().map(|f| f.new_path.as_str()).collect();
    assert_eq!(paths, ["subA", "subB", "top.txt"]);
    assert_eq!(
        files[1].nested_submodule_diff.as_ref().unwrap()[0].new_path,
        "subB/y"
    );
}

#[test]
fn binary_file_with_spaces_keeps_full_path() {
    let src = "diff --git a/my file.png b/my file.png\n\
               index 1111111..2222222 100644\n\
               Binary files a/my file.png and b/my file.png differ\n";
    let files = parse_unified_diff(src);
    assert_eq!(files[0].old_path, "my file.png");
    assert_eq!(files[0].new_path, "my file.png");
    assert!(files[0].is_binary());
}

#[test]
fn mode_change_with_spaces_keeps_full_path() {
    let src = "diff --git a/my script.sh b/my script.sh\nold mode 100644\nnew mode 100755\n";
    let files = parse_unified_diff(src);
    assert_eq!(files[0].old_path, "my script.sh");
    assert_eq!(files[0].new_path, "my script.sh");
}

#[test]
fn quoted_paths_are_decoded() {
    let src = "diff --git \"a/caf\\303\\251.md\" \"b/caf\\303\\251.md\"\n\
               --- \"a/caf\\303\\251.md\"\n\
               +++ \"b/caf\\303\\251.md\"\n\
               @@ -1 +1 @@\n-a\n+b\n";
    let files = parse_unified_diff(src);
    assert_eq!(files[0].old_path, "café.md");
    assert_eq!(files[0].new_path, "café.md");
}

#[test]
fn quoted_rename_is_decoded() {
    let src = "diff --git \"a/\\303\\251.md\" \"b/\\303\\251 2.md\"\n\
               similarity index 100%\n\
               rename from \"\\303\\251.md\"\n\
               rename to \"\\303\\251 2.md\"\n";
    let files = parse_unified_diff(src);
    assert!(matches!(
        &files[0].change,
        FileChange::Renamed { from, to, .. } if from == "é.md" && to == "é 2.md"
    ));
}

#[test]
fn added_binary_keeps_added_status() {
    let src = "diff --git a/i.png b/i.png\nnew file mode 100644\n\
               index 0000000..abcdef1\nBinary files /dev/null and b/i.png differ\n";
    let files = parse_unified_diff(src);
    assert!(matches!(files[0].change, FileChange::Added));
    assert!(files[0].is_binary());
    assert_eq!(files[0].new_path, "i.png");
}

#[test]
fn format_patch_signature_is_not_a_hunk_line() {
    let src = "diff --git a/x b/x\n--- a/x\n+++ b/x\n@@ -1 +1 @@\n-a\n+b\n-- \n2.43.0\n\n";
    let files = parse_unified_diff(src);
    assert_eq!(
        files[0].hunks[0].lines,
        vec![HunkLine::Del("a".into()), HunkLine::Add("b".into())]
    );
}

#[test]
fn removed_line_starting_with_dashes_stays_in_hunk() {
    let src = "diff --git a/x b/x\n--- a/x\n+++ b/x\n@@ -1,2 +1,1 @@\n--- old\n keep\n";
    let files = parse_unified_diff(src);
    assert_eq!(
        files[0].hunks[0].lines,
        vec![
            HunkLine::Del("-- old".into()),
            HunkLine::Context("keep".into())
        ]
    );
    assert_eq!(files[0].old_path, "x");
}

#[test]
fn no_newline_marker_after_last_counted_line_is_kept() {
    let src =
        "diff --git a/x b/x\n--- a/x\n+++ b/x\n@@ -1 +1 @@\n-a\n+b\n\\ No newline at end of file\n";
    let files = parse_unified_diff(src);
    assert_eq!(files[0].hunks[0].lines.last(), Some(&HunkLine::NoNewline));
}

#[test]
fn word_diff_bare_context_lines_stay_in_hunk() {
    let src = "diff --git a/f.txt b/f.txt\n--- a/f.txt\n+++ b/f.txt\n@@ -1,4 +1,5 @@\n\
               a [-b-]{+B+} c\nkeep\nx y {+z+}\nlast\n{+new line+}\n";
    let files = parse_unified_diff(src);
    let lines = &files[0].hunks[0].lines;
    assert_eq!(lines.len(), 5, "{lines:?}");
    assert_eq!(lines[1], HunkLine::Context("keep".into()));
    assert!(matches!(lines[4], HunkLine::WordDiff(_)));
}

#[test]
fn plain_unified_diff_is_parsed() {
    let src = "--- a.txt\t2024-01-01 10:00:00\n+++ b.txt\t2024-01-01 10:00:01\n\
               @@ -1 +1 @@\n-a\n+b\n\
               --- c.txt\t2024-01-01 10:00:00\n+++ c.txt\t2024-01-01 10:00:01\n\
               @@ -1 +1 @@\n-c\n+d\n";
    let files = parse_unified_diff(src);
    assert_eq!(files.len(), 2);
    assert_eq!(files[0].old_path, "a.txt");
    assert_eq!(files[0].new_path, "b.txt");
    assert_eq!(files[1].new_path, "c.txt");
    assert_eq!(files[1].hunks[0].lines.len(), 2);
}

const WORD_DIFF_HEAD: &str =
    "diff --git a/a.rs b/a.rs\nindex 871539c..9fc8cac 100644\n--- a/a.rs\n+++ b/a.rs\n@@ -1,4 +1,5 @@\n";

#[test]
fn indented_word_diff_lines_keep_their_content() {
    let src = format!(
        "{WORD_DIFF_HEAD}fn main() {{\n    let x = [-1;-]{{+10;+}}\n    let y = 2;\n    {{+let z = 3;+}}\n}}\n"
    );
    let lines = &parse_unified_diff(&src)[0].hunks[0].lines;
    assert_eq!(lines.len(), 5, "{lines:?}");
    assert_eq!(lines[2], HunkLine::Context("    let y = 2;".into()));
    let HunkLine::WordDiff(segments) = &lines[3] else {
        panic!("{:?}", lines[3]);
    };
    assert_eq!(segments[0].text, "    ");
    assert_eq!(lines[4], HunkLine::Context("}".into()));
}

#[test]
fn porcelain_word_diff_is_assembled_per_line() {
    let src = format!(
        "{WORD_DIFF_HEAD} fn main() {{\n~\n     let x = \n-1;\n+10;\n~\n     let y = 2;\n~\n     \n+let z = 3;\n~\n }}\n~\n"
    );
    let lines = &parse_unified_diff(&src)[0].hunks[0].lines;
    assert_eq!(lines.len(), 5, "{lines:?}");
    assert_eq!(lines[0], HunkLine::Context("fn main() {".into()));
    assert!(matches!(&lines[1], HunkLine::WordDiff(s) if s.len() == 3));
    assert_eq!(lines[4], HunkLine::Context("}".into()));
}

#[test]
fn literal_word_markers_in_a_normal_diff_stay_counted() {
    let src = "diff --git a/w.patch b/w.patch\n--- a/w.patch\n+++ b/w.patch\n@@ -1 +1 @@\n\
               -foo [-bar-]\n+foo {+bar+}\n";
    let lines = &parse_unified_diff(src)[0].hunks[0].lines;
    assert_eq!(
        lines,
        &vec![
            HunkLine::Del("foo [-bar-]".into()),
            HunkLine::Add("foo {+bar+}".into())
        ]
    );
}

#[test]
fn binary_name_containing_and_keeps_its_path() {
    let src = "diff --git a/gone and done.png b/gone and done.png\ndeleted file mode 100644\n\
               index 1111111..0000000\nBinary files a/gone and done.png and /dev/null differ\n";
    let file = &parse_unified_diff(src)[0];
    assert_eq!(file.old_path, "gone and done.png");
    assert!(matches!(file.change, FileChange::Deleted));
    assert!(file.is_binary());
}

#[test]
fn gnu_diff_recursive_output_is_split_per_file() {
    let src = "diff -ruN a/added b/added\n--- a/added\t1970-01-01 00:00:00.000000000 +0000\n\
               +++ b/added\t2026-10-14 21:18:03.431946889 +0000\n@@ -0,0 +1 @@\n+n\n\
               diff -ruN a/f b/f\n--- a/f\t2026-10-14 21:18:03.431946889 +0000\n\
               +++ b/f\t2026-10-14 21:18:03.431946889 +0000\n@@ -1 +1 @@\n-old\n+new\n";
    let files = parse_unified_diff(src);
    assert_eq!(files.len(), 2);
    assert!(matches!(files[0].change, FileChange::Added));
    assert_eq!(files[0].new_path, "added");
    assert_eq!(files[1].header_lines[0], "diff -ruN a/f b/f");
    assert!(files[0].header_lines.iter().all(|l| !l.contains("a/f b/f")));
}

#[test]
fn stripped_blank_context_does_not_switch_to_word_diff() {
    let log = "commit 0123456789abcdef0123456789abcdef01234567\nAuthor: a <a@a>\n\n    msg\n\n\
               diff --git a/x b/x\n--- a/x\n+++ b/x\n@@ -1,3 +1,3 @@\n a\n\n-b\n+c\n\
               commit 89abcdef0123456789abcdef0123456789abcdef\nAuthor: a <a@a>\n";
    let lines = &parse_unified_diff(log)[0].hunks[0].lines;
    assert!(lines.contains(&HunkLine::Del("b".into())), "{lines:?}");
    assert!(lines.contains(&HunkLine::Add("c".into())), "{lines:?}");

    let patch =
        "diff --git a/x b/x\n--- a/x\n+++ b/x\n@@ -1,3 +1,3 @@\n a\n\n-b\n+c\n-- \n2.43.0\n";
    let lines = &parse_unified_diff(patch)[0].hunks[0].lines;
    assert!(lines.contains(&HunkLine::Del("b".into())), "{lines:?}");
}

#[test]
fn word_markers_in_a_commit_message_do_not_switch_mode() {
    let src = "From 0123456789abcdef0123456789abcdef01234567 Mon Sep 17 00:00:00 2001\n\
               Subject: [PATCH] use {+x+} markers\n\nexplain [-old-] {+new+}\n---\n\
               diff --git a/x b/x\n--- a/x\n+++ b/x\n@@ -1 +1 @@\n-a\n+b\n";
    let lines = &parse_unified_diff(src)[0].hunks[0].lines;
    assert_eq!(
        lines,
        &vec![HunkLine::Del("a".into()), HunkLine::Add("b".into())]
    );
}

#[test]
fn porcelain_single_line_hunk_is_detected() {
    let src = "diff --git a/x b/x\n--- a/x\n+++ b/x\n@@ -1 +1 @@\n-a\n+b\n~\n";
    let lines = &parse_unified_diff(src)[0].hunks[0].lines;
    assert!(
        matches!(&lines[0], HunkLine::WordDiff(s) if s.len() == 2),
        "{lines:?}"
    );
}

#[test]
fn epoch_timestamps_mark_added_files_in_any_timezone() {
    for stamp in [
        "1970-01-01 00:00:00.000000000 +0000",
        "1969-12-31 19:00:00.000000000 -0500",
        "1970-01-01 09:00:00.000000000 +0900",
    ] {
        let src = format!(
            "--- a/new\t{stamp}\n+++ b/new\t2026-10-14 21:18:03.431946889 +0000\n@@ -0,0 +1 @@\n+n\n"
        );
        let files = parse_unified_diff(&src);
        assert!(matches!(files[0].change, FileChange::Added), "{stamp}");
    }
    let src = "--- a/f\t1970-01-01 01:00:00 +0000\n+++ b/f\t2026-01-01 00:00:00 +0000\n@@ -1 +1 @@\n-a\n+b\n";
    assert!(matches!(
        parse_unified_diff(src)[0].change,
        FileChange::Modified
    ));
}

#[test]
fn diff_command_lines_need_an_option_and_an_operand() {
    let prose = "diff between v1 and v2 follows\n--- a.txt\n+++ b.txt\n@@ -1 +1 @@\n-a\n+b\n";
    let files = parse_unified_diff(prose);
    assert_eq!(files.len(), 1);
    assert!(files[0]
        .header_lines
        .iter()
        .all(|l| !l.starts_with("diff between")));

    let hg = "diff -r 1234abcd -r 5678ef01 a.txt\n--- a/a.txt\n+++ b/a.txt\n@@ -1 +1 @@\n-a\n+b\n\
              diff -r 1234abcd -r 5678ef01 b.txt\n--- a/b.txt\n+++ b/b.txt\n@@ -1 +1 @@\n-c\n+d\n";
    let files = parse_unified_diff(hg);
    assert_eq!(files.len(), 2);
    assert_eq!(
        files[1].header_lines[0],
        "diff -r 1234abcd -r 5678ef01 b.txt"
    );
}

#[test]
fn merge_conflict_with_two_files_stays_counted() {
    let src = "diff --cc a.txt\nindex 3659f2c,1abd9d7..0000000\n--- a/a.txt\n+++ b/a.txt\n\
               @@@ -1,3 -1,4 +1,8 @@@\n  A\n++<<<<<<< HEAD\n +m2\n++=======\n+ m\n+ x1\n\
               ++>>>>>>> x\n  B\n\
               diff --cc b.txt\nindex 0c3820b,fef2f8a..0000000\n--- a/b.txt\n+++ b/b.txt\n\
               @@@ -1,3 -1,3 +1,7 @@@\n  C\n++<<<<<<< HEAD\n +y2\n++=======\n+ x2\n++>>>>>>> x\n  D\n";
    let files = parse_unified_diff(src);
    assert_eq!(files.len(), 2);
    for file in &files {
        let lines = &file.hunks[0].lines;
        assert!(
            lines
                .iter()
                .any(|l| matches!(l, HunkLine::Combined(p, _) if p == "++")),
            "{lines:?}"
        );
        assert!(!lines.iter().any(|l| matches!(l, HunkLine::WordDiff(_))));
    }
}

#[test]
fn wrong_hunk_count_before_next_file_stays_counted() {
    let src = "diff --git a/x b/x\n--- a/x\n+++ b/x\n@@ -1,3 +1,3 @@\n-a\n+b\n\
               diff --git a/y b/y\n--- a/y\n+++ b/y\n@@ -1 +1 @@\n-c\n+d\n";
    let files = parse_unified_diff(src);
    assert_eq!(files.len(), 2);
    assert_eq!(
        files[1].hunks[0].lines,
        vec![HunkLine::Del("c".into()), HunkLine::Add("d".into())]
    );
}

#[test]
fn fully_prefixed_word_diff_is_detected() {
    let head = "diff --git a/f b/f\nindex 157afff..848beaf 100644\n--- a/f\n+++ b/f\n";
    for hunk in [
        "@@ -3 +3 @@ fn main() {\n    let b = [-2;-]{+20;+}\n",
        "@@ -1 +1 @@\n    [-one-]{+two+}\n",
    ] {
        let lines = &parse_unified_diff(&format!("{head}{hunk}"))[0].hunks[0].lines;
        let HunkLine::WordDiff(segments) = &lines[0] else {
            panic!("{lines:?}");
        };
        assert!(segments[0].text.starts_with("    "), "{segments:?}");
        assert!(segments.iter().any(|s| s.kind == WordDiffKind::Del));
    }
    let bullets = format!("{head}@@ -1,3 +1,3 @@\n- alpha\n- [-beta-]{{+BETA+}}\n- gamma\n");
    let lines = &parse_unified_diff(&bullets)[0].hunks[0].lines;
    assert_eq!(lines.len(), 3, "{lines:?}");
    assert_eq!(lines[0], HunkLine::Context("- alpha".into()));
    assert!(matches!(&lines[1], HunkLine::WordDiff(s) if s[0].text == "- "));
}

#[test]
fn literal_word_markers_in_context_of_a_normal_diff_stay_counted() {
    let src = "diff --git a/w b/w\n--- a/w\n+++ b/w\n@@ -1,2 +1,2 @@\n \
               see [-x-]{+y+}\n-old\n+new [-x-]{+y+}\n";
    let lines = &parse_unified_diff(src)[0].hunks[0].lines;
    assert_eq!(
        lines,
        &vec![
            HunkLine::Context("see [-x-]{+y+}".into()),
            HunkLine::Del("old".into()),
            HunkLine::Add("new [-x-]{+y+}".into()),
        ]
    );
}

#[test]
fn malformed_epoch_stamps_do_not_panic() {
    let parse = |stamp: &str| {
        let src =
            format!("--- a/f\t{stamp}\n+++ b/f\t2026-01-01 00:00:00 +0000\n@@ -0,0 +1 @@\n+n\n");
        parse_unified_diff(&src)[0].change.clone()
    };
    assert!(matches!(
        parse("1970-01-01 00:00:00 +0é1"),
        FileChange::Added
    ));
    assert!(!matches!(
        parse("1969-12-31 19:00:00 +0é1"),
        FileChange::Added
    ));
    assert!(!matches!(
        parse("99999999999999999-01-01 00:00:00 +0000"),
        FileChange::Added
    ));
    assert!(!matches!(
        parse("1970-01-01 99999999999:00:00 +0000"),
        FileChange::Added
    ));
}

#[test]
fn double_dash_content_ending_a_hunk_is_not_a_file_header() {
    let src = "diff --git a/q.sql b/q.sql\n--- a/q.sql\n+++ b/q.sql\n@@ -4,2 +4 @@ k3\n\
               -a [-1-]\n--- old\n+++ new\n@@ -12 +11 @@ k9\n-z1\n+z2\n";
    let hunks = &parse_unified_diff(src)[0].hunks;
    assert_eq!(
        hunks[0].lines,
        vec![
            HunkLine::Del("a [-1-]".into()),
            HunkLine::Del("-- old".into()),
            HunkLine::Add("++ new".into()),
        ]
    );
    assert_eq!(
        hunks[1].lines,
        vec![HunkLine::Del("z1".into()), HunkLine::Add("z2".into())]
    );
}

#[test]
fn porcelain_outlasting_a_one_line_hunk_is_detected() {
    let head = "diff --git a/f b/f\n--- a/f\n+++ b/f\n";
    for hunk in [
        "@@ -3 +3 @@ fn main() {\n     let b = \n-2;\n+20;\n~\n",
        "@@ -1 +1 @@\n     \n-one\n+two\n~\n",
    ] {
        let lines = &parse_unified_diff(&format!("{head}{hunk}"))[0].hunks[0].lines;
        assert_eq!(lines.len(), 1, "{lines:?}");
        let HunkLine::WordDiff(segments) = &lines[0] else {
            panic!("{lines:?}");
        };
        assert_eq!(segments.len(), 3, "{segments:?}");
        assert_eq!(segments[1].kind, WordDiffKind::Del);
        assert_eq!(segments[2].kind, WordDiffKind::Add);
    }
}
