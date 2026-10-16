use crate::app::mode::{
    detect_mode, detect_mode_from_bytes, detect_mode_from_content, is_diff_extension, AppMode,
};
use crate::app::DiffSource;
use crate::cli::{is_diff_spec, parse_cli, parse_diff_spec};
use std::path::PathBuf;

#[test]
fn detects_diff_git_prefix_bytes() {
    let buf = b"diff --git a/foo b/foo\nindex 000..111 100644\n--- a/foo\n+++ b/foo\n";
    assert_eq!(detect_mode_from_bytes(buf), AppMode::Diff);
}

#[test]
fn detects_diff_cc_prefix_bytes() {
    let buf = b"diff --cc file.txt\n";
    assert_eq!(detect_mode_from_bytes(buf), AppMode::Diff);
}

#[test]
fn detects_diff_combined_prefix_bytes() {
    let buf = b"diff --combined file.txt\n";
    assert_eq!(detect_mode_from_bytes(buf), AppMode::Diff);
}

#[test]
fn ignores_diff_signature_inside_a_fenced_block() {
    let buf = b"# Heading\n\n```diff\ndiff --git a/x b/x\n--- a/x\n+++ b/x\n@@ -1 +1 @@\n```\n";
    assert_eq!(detect_mode_from_bytes(buf), AppMode::Document);
}

#[test]
fn finds_a_diff_after_a_preamble_on_stdin() {
    let stat = b" src/a.rs | 2 +-\n 1 file changed\n\ndiff --git a/src/a.rs b/src/a.rs\n";
    assert_eq!(detect_mode_from_bytes(stat), AppMode::Diff);
    let gnu = b"Only in b: new\ndiff -ruN a/f b/f\n--- a/f\t2024-01-01\n+++ b/f\t2024-01-01\n@@ -1 +1 @@\n";
    assert_eq!(detect_mode_from_bytes(gnu), AppMode::Diff);
    let svn =
        b"Index: f.txt\n===================================================================\n\
                --- f.txt\t(revision 1)\n+++ f.txt\t(working copy)\n@@ -1 +1 @@\n";
    assert_eq!(detect_mode_from_bytes(svn), AppMode::Diff);
}

#[test]
fn markdown_files_are_never_detected_as_diffs() {
    let src = "commit 0123456789abcdef0123456789abcdef01234567\n";
    assert_eq!(
        detect_mode(Some(std::path::Path::new("note.md")), false, src),
        AppMode::Document
    );
    assert_eq!(
        detect_mode_from_content("commit 1234567 was the culprit\n"),
        AppMode::Document
    );
}

#[test]
fn skips_blank_lines_bytes() {
    let buf = b"\n\n   \ndiff --git a/x b/x\n";
    assert_eq!(detect_mode_from_bytes(buf), AppMode::Diff);
}

#[test]
fn tolerates_non_utf8_bytes_after_signature() {
    let mut buf = Vec::from(&b"diff --git a/x b/x\n"[..]);
    buf.extend_from_slice(&[0xC3, 0x28, 0xFF, 0xFE]);
    assert_eq!(detect_mode_from_bytes(&buf), AppMode::Diff);
}

#[test]
fn detects_diff_from_str_content() {
    let src = "diff --git a/foo b/foo\nindex 000..111 100644\n";
    assert_eq!(detect_mode_from_content(src), AppMode::Diff);
}

#[test]
fn no_false_positive_on_changelog_with_diff_fence() {
    let src = "# Changelog\n\n## v1\n\n```diff\ndiff --git a/foo b/foo\n```\n";
    assert_eq!(detect_mode_from_content(src), AppMode::Document);
}

#[test]
fn no_false_positive_on_readme_with_yaml_frontmatter() {
    let src = "---\ntitle: leaf\n---\n\n# leaf\n\nSee `git diff --git`\n";
    assert_eq!(detect_mode_from_content(src), AppMode::Document);
}

#[test]
fn no_false_positive_on_blog_talking_about_git() {
    let src = "Some prose about `git diff --git a/x b/x` embedded inline.\n";
    assert_eq!(detect_mode_from_content(src), AppMode::Document);
}

#[test]
fn empty_input_is_document() {
    assert_eq!(detect_mode_from_bytes(b""), AppMode::Document);
    assert_eq!(detect_mode_from_content(""), AppMode::Document);
}

#[test]
fn only_blank_lines_is_document() {
    assert_eq!(detect_mode_from_bytes(b"\n\n \t\n"), AppMode::Document);
    assert_eq!(detect_mode_from_content("\n\n  \t\n"), AppMode::Document);
}

#[test]
fn detect_mode_cli_forced_wins() {
    let src = "# heading\n";
    assert_eq!(detect_mode(None, true, src), AppMode::Diff);
}

#[test]
fn detect_mode_diff_extension_wins_over_content() {
    let src = "# not really a diff\n";
    let path = PathBuf::from("foo.diff");
    assert_eq!(detect_mode(Some(&path), false, src), AppMode::Diff);
}

#[test]
fn detect_mode_patch_extension() {
    let path = PathBuf::from("foo.patch");
    assert_eq!(detect_mode(Some(&path), false, ""), AppMode::Diff);
}

#[test]
fn detect_mode_content_sniff_when_no_hint() {
    let src = "diff --git a/x b/x\n";
    assert_eq!(detect_mode(None, false, src), AppMode::Diff);
}

#[test]
fn detect_mode_md_file_with_diff_fence_stays_document() {
    let src = "# Notes\n\n```\ndiff --git a/x b/x\n```\n";
    let path = PathBuf::from("README.md");
    assert_eq!(detect_mode(Some(&path), false, src), AppMode::Document);
}

#[test]
fn is_diff_extension_matches_diff_and_patch() {
    assert!(is_diff_extension("diff"));
    assert!(is_diff_extension("patch"));
    assert!(is_diff_extension("DIFF"));
    assert!(is_diff_extension("Patch"));
    assert!(!is_diff_extension("md"));
    assert!(!is_diff_extension(""));
}

#[test]
fn is_diff_spec_accepts_source_keywords() {
    assert!(is_diff_spec("cached"));
    assert!(is_diff_spec("staged"));
    assert!(is_diff_spec("working"));
    assert!(is_diff_spec("HEAD"));
    assert!(is_diff_spec("main..topic"));
    assert!(is_diff_spec("origin/feature"));
}

#[test]
fn is_diff_spec_rejects_flags_and_empty() {
    assert!(!is_diff_spec(""));
    assert!(!is_diff_spec("--watch"));
    assert!(!is_diff_spec("-w"));
}

#[test]
fn parse_diff_spec_recognizes_cached_and_staged() {
    assert_eq!(
        parse_diff_spec("cached").unwrap().source,
        DiffSource::Cached
    );
    assert_eq!(
        parse_diff_spec("staged").unwrap().source,
        DiffSource::Cached
    );
}

#[test]
fn parse_diff_spec_recognizes_working() {
    assert_eq!(
        parse_diff_spec("working").unwrap().source,
        DiffSource::Working
    );
}

#[test]
fn parse_diff_spec_treats_other_as_ref() {
    assert_eq!(
        parse_diff_spec("HEAD~2").unwrap().source,
        DiffSource::Ref("HEAD~2".to_string())
    );
    assert_eq!(
        parse_diff_spec("main..topic").unwrap().source,
        DiffSource::Ref("main..topic".to_string())
    );
}

#[test]
fn parse_diff_spec_rejects_empty() {
    assert!(parse_diff_spec("").is_err());
    assert!(parse_diff_spec("   ").is_err());
}

#[test]
fn parse_cli_accepts_diff_no_spec() {
    let args = vec!["leaf".into(), "--diff".into()];
    let options = parse_cli(&args).unwrap();
    let spec = options.diff.unwrap();
    assert_eq!(spec.source, DiffSource::Working);
}

#[test]
fn parse_cli_accepts_diff_cached_spec() {
    let args = vec!["leaf".into(), "--diff".into(), "cached".into()];
    let options = parse_cli(&args).unwrap();
    assert_eq!(options.diff.unwrap().source, DiffSource::Cached);
}

#[test]
fn parse_cli_accepts_diff_equals_form() {
    let args = vec!["leaf".into(), "--diff=cached".into()];
    let options = parse_cli(&args).unwrap();
    assert_eq!(options.diff.unwrap().source, DiffSource::Cached);
}

#[test]
fn parse_cli_accepts_diff_with_positional() {
    let args = vec![
        "leaf".into(),
        "--diff".into(),
        "cached".into(),
        "src/foo.rs".into(),
    ];
    let options = parse_cli(&args).unwrap();
    assert_eq!(options.diff.unwrap().source, DiffSource::Cached);
    assert_eq!(options.file_arg.as_deref(), Some("src/foo.rs"));
}

#[test]
fn parse_cli_diff_ref_is_accepted() {
    let args = vec!["leaf".into(), "--diff=HEAD~2".into()];
    let options = parse_cli(&args).unwrap();
    assert_eq!(
        options.diff.unwrap().source,
        DiffSource::Ref("HEAD~2".to_string())
    );
}

#[test]
fn parse_cli_rejects_diff_with_watch() {
    let args = vec!["leaf".into(), "--diff".into(), "--watch".into()];
    let err = parse_cli(&args).unwrap_err().to_string();
    assert!(
        err.contains("--diff cannot be combined with --watch"),
        "{err}"
    );
}

#[test]
fn parse_cli_rejects_diff_with_inline() {
    let args = vec!["leaf".into(), "--diff".into(), "--inline".into()];
    let err = parse_cli(&args).unwrap_err().to_string();
    assert!(
        err.contains("--diff cannot be combined with --inline"),
        "{err}"
    );
}

#[test]
fn parse_cli_rejects_diff_with_picker() {
    let args = vec!["leaf".into(), "--diff".into(), "--picker".into()];
    let err = parse_cli(&args).unwrap_err().to_string();
    assert!(
        err.contains("--diff cannot be combined with --picker"),
        "{err}"
    );
}

#[test]
fn parse_cli_rejects_diff_with_fuzzy() {
    let args = vec!["leaf".into(), "--diff".into(), "--fuzzy".into()];
    let err = parse_cli(&args).unwrap_err().to_string();
    assert!(
        err.contains("--diff cannot be combined with --fuzzy"),
        "{err}"
    );
}

#[test]
fn git_show_and_format_patch_headers_are_diffs() {
    let show = b"commit 0123456789abcdef0123456789abcdef01234567\nAuthor: a <a@a>\n\n    msg\n";
    assert_eq!(detect_mode_from_bytes(show), AppMode::Diff);
    let patch = b"From 0123456789abcdef0123456789abcdef01234567 Mon Sep 17 00:00:00 2001\n";
    assert_eq!(detect_mode_from_bytes(patch), AppMode::Diff);
}

#[test]
fn plain_unified_diff_is_detected() {
    let src = "--- a.txt\t2024-01-01\n+++ b.txt\t2024-01-01\n@@ -1 +1 @@\n-a\n+b\n";
    assert_eq!(detect_mode_from_content(src), AppMode::Diff);
    assert_eq!(detect_mode_from_bytes(src.as_bytes()), AppMode::Diff);
}

#[test]
fn prose_starting_with_commit_or_from_stays_document() {
    assert_eq!(
        detect_mode_from_content("commit message guidelines\n"),
        AppMode::Document
    );
    assert_eq!(
        detect_mode_from_content("From the author\n"),
        AppMode::Document
    );
    assert_eq!(
        detect_mode_from_content("--- \ntitle: x\n---\n"),
        AppMode::Document
    );
}

#[test]
fn parse_cli_tracks_explicit_diff_spec() {
    let bare = parse_cli(&["leaf".into(), "--diff".into()]).unwrap();
    assert!(!bare.diff_spec_explicit);
    let named = parse_cli(&["leaf".into(), "--diff".into(), "working".into()]).unwrap();
    assert!(named.diff_spec_explicit);
    let equals = parse_cli(&["leaf".into(), "-d=HEAD".into()]).unwrap();
    assert!(equals.diff_spec_explicit);
}

#[test]
fn setext_index_heading_and_nested_fences_stay_documents() {
    let heading = b"Index: Terms\n============\n\nSome prose.\n";
    assert_eq!(detect_mode_from_bytes(heading), AppMode::Document);
    let nested =
        b"# Doc\n\n````md\n```diff\ndiff --git a/x b/x\n--- a/x\n+++ b/x\n@@ -1 +1 @@\n```\n````\n";
    assert_eq!(detect_mode_from_bytes(nested), AppMode::Document);
    let mixed = b"# Doc\n\n~~~\n```\ndiff --git a/x b/x\n~~~\n";
    assert_eq!(detect_mode_from_bytes(mixed), AppMode::Document);
}

#[test]
fn fence_indented_four_spaces_is_code_not_a_fence() {
    let src = b"# Doc\n\n    ```\n    code\n\ndiff --git a/x b/x\n--- a/x\n+++ b/x\n@@ -1 +1 @@\n-a\n+b\n";
    assert_eq!(detect_mode_from_bytes(src), AppMode::Diff);
    let tab = b"# Doc\n\n\t```\n\ndiff --git a/x b/x\n--- a/x\n+++ b/x\n@@ -1 +1 @@\n-a\n+b\n";
    assert_eq!(detect_mode_from_bytes(tab), AppMode::Diff);
}

#[test]
fn svn_header_needs_the_old_file_line_right_after_the_rule() {
    let svn = b"Some preamble\nIndex: f.txt\n===================================================================\n\
                --- f.txt\t(revision 1)\n+++ f.txt\t(working copy)\n@@ -1 +1 @@\n-a\n+b\n";
    assert_eq!(detect_mode_from_bytes(svn), AppMode::Diff);
    let far = b"Some preamble\nIndex: Terms\n============\n\n--- quote\n";
    assert_eq!(detect_mode_from_bytes(far), AppMode::Document);
}
