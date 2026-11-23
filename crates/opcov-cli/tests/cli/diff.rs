use std::fs;
use std::io::Write;
use std::process::{Command, Stdio};

use serde_json::Value;

use super::fixture::{BINARY, Extras, project, run, stderr, stdout};

fn gate_diff(name: &str, diff: &str, extra: &[&str]) -> std::process::Output {
    let (directory, report) = project(name, Extras::default());
    let patch = directory.join("change.diff");
    fs::write(&patch, diff).unwrap();
    let mut arguments = vec![
        "gate",
        "--report",
        report.to_str().unwrap(),
        "--base",
        directory.to_str().unwrap(),
        "--format",
        "json",
        "--diff",
        patch.to_str().unwrap(),
    ];
    arguments.extend_from_slice(extra);
    return run(&arguments);
}

fn keys(output: &std::process::Output) -> Vec<String> {
    let result: Value = serde_json::from_str(&stdout(output))
        .unwrap_or_else(|error| panic!("{error}: {}{}", stdout(output), stderr(output)));
    return result["files"]
        .as_array()
        .unwrap()
        .iter()
        .flat_map(|file| return file["functions"].as_array().unwrap().iter())
        .map(|function| return function["key"].as_str().unwrap().to_owned())
        .collect();
}

const HEADER: &str = "diff --git a/app/Foo.php b/app/Foo.php\nindex 1111111..2222222 100644\n--- a/app/Foo.php\n+++ b/app/Foo.php\n";

#[test]
fn a_modified_line_selects_its_function() {
    let diff = format!(
        "{HEADER}@@ -14 +14 @@ class Foo\n-            $label .= $item;\n+            $label .= $item . ',';\n"
    );
    assert_eq!(
        keys(&gate_diff("diff_modified", &diff, &[])),
        ["App\\Foo::bar"],
        "line 14"
    );
}

#[test]
fn a_deletion_selects_the_function_around_the_gap() {
    let diff = format!("{HEADER}@@ -21,1 +20,0 @@\n-        return $x * 3;\n");
    assert_eq!(
        keys(&gate_diff("diff_deleted_line", &diff, &[])),
        ["App\\Foo::baz"],
        "gap before 21"
    );
    let between = format!("{HEADER}@@ -18,2 +17,0 @@\n-\n-    public function old() {{}}\n");
    assert_eq!(
        keys(&gate_diff("diff_deleted_method", &between, &[])),
        ["{main}@app/Foo.php"],
        "a method deleted between two others touches only the file's top level"
    );
}

#[test]
fn context_lines_advance_the_new_side() {
    let diff = format!(
        "{HEADER}@@ -19,4 +19,4 @@\n     public function baz(int $x): int\n     {{\n-        return $x * 3;\n+        return $x * 2;\n     }}\n"
    );
    assert_eq!(
        keys(&gate_diff("diff_context", &diff, &[])),
        ["App\\Foo::baz"],
        "the + line is 21"
    );
}

#[test]
fn a_new_file_selects_every_function() {
    let source = fs::read_to_string(super::fixture::fixture_source()).unwrap();
    let added: String = source
        .lines()
        .map(|line| return format!("+{line}\n"))
        .collect();
    let diff = format!(
        "diff --git a/app/Foo.php b/app/Foo.php\nnew file mode 100644\nindex 0000000..2222222\n--- /dev/null\n+++ b/app/Foo.php\n@@ -0,0 +1,{} @@\n{added}",
        source.lines().count()
    );
    assert_eq!(
        keys(&gate_diff("diff_new_file", &diff, &[])).len(),
        5,
        "all five functions"
    );
}

#[test]
fn deleted_files_and_pure_renames_change_nothing() {
    let diff = format!(
        "diff --git a/app/Old.php b/app/Old.php\ndeleted file mode 100644\nindex 3333333..0000000\n--- a/app/Old.php\n+++ /dev/null\n@@ -1,2 +0,0 @@\n-<?php\n-+++ b/app/Foo.php\ndiff --git a/app/Same.php b/app/Moved.php\nsimilarity index 100%\nrename from app/Same.php\nrename to app/Moved.php\n{HEADER}@@ -21 +21 @@\n-        return $x * 3;\n+        return $x * 2;\n"
    );
    assert_eq!(
        keys(&gate_diff("diff_deleted_renamed", &diff, &[])),
        ["App\\Foo::baz"],
        "removed lines of a deleted file are no `+++` header, and a pure rename has no hunk"
    );
}

#[test]
fn a_renamed_file_is_checked_under_its_new_path() {
    let diff = "diff --git a/app/Bar.php b/app/Foo.php\nsimilarity index 90%\nrename from app/Bar.php\nrename to app/Foo.php\nindex 1111111..2222222 100644\n--- a/app/Bar.php\n+++ b/app/Foo.php\n@@ -21 +21 @@\n-        return $x * 3;\n+        return $x * 2;\n";
    assert_eq!(
        keys(&gate_diff("diff_renamed", diff, &[])),
        ["App\\Foo::baz"],
        "new path"
    );
}

#[test]
fn no_newline_markers_are_skipped() {
    let diff = format!(
        "{HEADER}@@ -29 +29 @@\n-}}\n\\ No newline at end of file\n+}}\n\\ No newline at end of file\n"
    );
    assert_eq!(
        keys(&gate_diff("diff_no_newline", &diff, &[])),
        ["App\\helper", "{fn}@app/Foo.php:27-27"],
        "line 29 closes helper"
    );
}

#[test]
fn quoted_paths_and_strip_levels_are_honoured() {
    let quoted = "--- \"a/app/F\\157o.php\"\n+++ \"b/app/F\\157o.php\"\n@@ -21 +21 @@\n-x\n+y\n";
    assert_eq!(
        keys(&gate_diff("diff_quoted", quoted, &[])),
        ["App\\Foo::baz"],
        "octal escape"
    );
    let bare = "--- app/Foo.php\t2026-01-01 00:00:00\n+++ app/Foo.php\t2026-01-02 00:00:00\n@@ -21 +21 @@\n-x\n+y\n";
    assert_eq!(
        keys(&gate_diff("diff_strip_zero", bare, &["--strip", "0"])),
        ["App\\Foo::baz"],
        "--strip 0 keeps the path and the timestamp is dropped"
    );
}

#[test]
fn reads_the_diff_from_standard_input() {
    let (directory, report) = project("diff_stdin", Extras::default());
    let mut child = Command::new(BINARY)
        .args([
            "gate",
            "--report",
            report.to_str().unwrap(),
            "--base",
            directory.to_str().unwrap(),
            "--diff",
            "-",
        ])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .unwrap();
    let diff = format!("{HEADER}@@ -21 +21 @@\n-x\n+y\n");
    child
        .stdin
        .take()
        .unwrap()
        .write_all(diff.as_bytes())
        .unwrap();
    let output = child.wait_with_output().unwrap();
    assert_eq!(output.status.code(), Some(1), "baz is uncovered");
    assert!(
        stdout(&output).contains("uncovered lines: 21"),
        "{}",
        stdout(&output)
    );
}

#[test]
fn malformed_diffs_are_rejected_with_their_line() {
    let early = "@@ -1 +1 @@\n-a\n+b\n";
    let output = gate_diff("diff_early_hunk", early, &[]);
    assert_eq!(output.status.code(), Some(2), "a hunk needs a file header");
    assert!(
        stderr(&output).contains("line 1: hunk before a `+++` file header"),
        "{}",
        stderr(&output)
    );
    let truncated = format!("{HEADER}@@ -21,2 +21,2 @@\n-x\n+y\n");
    let output = gate_diff("diff_truncated", &truncated, &[]);
    assert_eq!(output.status.code(), Some(2), "a truncated hunk is invalid");
    assert!(
        stderr(&output).contains("the diff ends inside a hunk"),
        "{}",
        stderr(&output)
    );
    let bad_header = format!("{HEADER}@@ -a +1 @@\n");
    let output = gate_diff("diff_bad_header", &bad_header, &[]);
    assert_eq!(
        output.status.code(),
        Some(2),
        "a malformed header is invalid"
    );
    assert!(
        stderr(&output).contains("line 5: malformed hunk header"),
        "{}",
        stderr(&output)
    );
}
