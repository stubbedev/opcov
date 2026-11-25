use std::fmt::Write as _;
use std::fs;
use std::path::Path;
use std::process::Output;

use serde_json::Value;

use super::fixture::{Extras, SOURCE, fixture_source, project, run, stderr, stdout};

fn lines(text: &str) -> Vec<&str> {
    return text.split_inclusive('\n').collect();
}

fn diff_line(out: &mut String, sign: char, line: &str) {
    out.push(sign);
    if let Some(body) = line.strip_suffix('\n') {
        out.push_str(body);
        out.push('\n');
    } else {
        out.push_str(line);
        out.push_str("\n\\ No newline at end of file\n");
    }
}

fn unified(path: &str, old: Option<&str>, new: Option<&str>) -> String {
    let (before, after) = (lines(old.unwrap_or("")), lines(new.unwrap_or("")));
    let prefix = before
        .iter()
        .zip(&after)
        .take_while(|(left, right)| return left == right)
        .count();
    let suffix = before[prefix..]
        .iter()
        .rev()
        .zip(after[prefix..].iter().rev())
        .take_while(|(left, right)| return left == right)
        .count();
    let removed = &before[prefix..before.len().saturating_sub(suffix)];
    let added = &after[prefix..after.len().saturating_sub(suffix)];
    let start = |count: usize| {
        return if count == 0 {
            prefix
        } else {
            prefix.saturating_add(1)
        };
    };
    let (minus, plus) = (
        old.map_or_else(
            || return "/dev/null".to_owned(),
            |_| return format!("a/{path}"),
        ),
        new.map_or_else(
            || return "/dev/null".to_owned(),
            |_| return format!("b/{path}"),
        ),
    );
    let mut out = format!("diff --git a/{path} b/{path}\n");
    if old.is_none() {
        out.push_str("new file mode 100644\n");
    }
    if new.is_none() {
        out.push_str("deleted file mode 100644\n");
    }
    writeln!(out, "--- {minus}\n+++ {plus}").unwrap();
    if removed.is_empty() && added.is_empty() {
        return out;
    }
    writeln!(
        out,
        "@@ -{},{} +{},{} @@",
        start(removed.len()),
        removed.len(),
        start(added.len()),
        added.len()
    )
    .unwrap();
    for line in removed {
        diff_line(&mut out, '-', line);
    }
    for line in added {
        diff_line(&mut out, '+', line);
    }
    return out;
}

struct Change<'a> {
    path: &'a str,
    old: Option<&'a str>,
    new: Option<&'a str>,
}

fn base_source() -> String {
    return fs::read_to_string(fixture_source()).unwrap();
}

fn write(directory: &Path, path: &str, content: Option<&str>) {
    let target = directory.join(path);
    match content {
        Some(content) => {
            fs::create_dir_all(target.parent().unwrap()).unwrap();
            fs::write(target, content).unwrap();
        }
        None => fs::remove_file(target).unwrap(),
    }
}

fn impact(name: &str, setup: &[(&str, &str)], changes: &[Change<'_>], extra: &[&str]) -> Output {
    let (directory, report) = project(name, Extras::default());
    for (path, content) in setup {
        write(&directory, path, Some(content));
    }
    let mut diff = String::new();
    for change in changes {
        write(&directory, change.path, change.new);
        diff.push_str(&unified(change.path, change.old, change.new));
    }
    let patch = directory.join("change.diff");
    fs::write(&patch, diff).unwrap();
    let mut arguments = vec![
        "impact",
        "--report",
        report.to_str().unwrap(),
        "--base",
        directory.to_str().unwrap(),
        "--diff",
        patch.to_str().unwrap(),
    ];
    arguments.extend_from_slice(extra);
    return run(&arguments);
}

fn json(output: &Output) -> Value {
    return serde_json::from_str(&stdout(output))
        .unwrap_or_else(|error| panic!("{error}: {}{}", stdout(output), stderr(output)));
}

fn edited(from: &str, to: &str) -> String {
    let source = base_source();
    assert!(source.contains(from), "the fixture contains {from:?}");
    return source.replacen(from, to, 1);
}

fn foo<'a>(base: &'a str, new: &'a str) -> Change<'a> {
    return Change {
        path: SOURCE,
        old: Some(base),
        new: Some(new),
    };
}

const TEST_CLASS: &str =
    "<?php\n\nnamespace T;\n\nclass A\n{\n    public function one(): void\n    {\n    }\n}\n";
const HELPER: &str = "<?php\n\nnamespace T;\n\nclass Helper\n{\n    public function make(): int\n    {\n        return 1;\n    }\n}\n";

#[test]
fn a_body_change_selects_the_tests_that_reached_the_function() {
    let base = base_source();
    let new = edited("$label .= $item;", "$label .= $item . ',';");
    let output = impact(
        "impact_body",
        &[],
        &[foo(&base, &new)],
        &["--format", "json"],
    );
    assert_eq!(output.status.code(), Some(0), "{}", stderr(&output));
    let result = json(&output);
    assert_eq!(result["verdict"], "bounded", "verdict");
    assert_eq!(
        result["tests"],
        serde_json::json!(["T\\A::one", "T\\B::two"]),
        "tests"
    );
    let function = &result["changes"][0]["functions"][0];
    assert_eq!(function["key"], "App\\Foo::bar", "the changed function");
    assert_eq!(
        function["lines"],
        serde_json::json!({"covered": 6, "total": 6}),
        "lines"
    );
    assert_eq!(
        function["outcomes"],
        serde_json::json!({"covered": 5, "total": 6}),
        "outcomes"
    );
}

#[test]
fn an_uncovered_function_selects_no_test() {
    let base = base_source();
    let new = edited("return $x * 2;", "return $x * 3;");
    let output = impact("impact_uncovered", &[], &[foo(&base, &new)], &[]);
    assert_eq!(output.status.code(), Some(0), "{}", stderr(&output));
    let text = stdout(&output);
    assert!(
        text.contains("  App\\Foo::baz (base lines 19-22): 0/1 lines and 0/0 outcomes covered, reached by 0 test(s)\n"),
        "{text}"
    );
    assert!(
        text.ends_with("impact: run 0 test(s) and 0 test class(es)\n"),
        "{text}"
    );
    let filter = impact(
        "impact_uncovered_filter",
        &[],
        &[foo(&base, &new)],
        &["--format", "filter"],
    );
    assert_eq!(filter.status.code(), Some(0), "{}", stderr(&filter));
    assert_eq!(stdout(&filter), "", "no test to run");
}

#[test]
fn a_closure_change_selects_only_the_closure() {
    let base = base_source();
    let new = edited("$y + 1;", "$y + 2;");
    let output = impact(
        "impact_closure",
        &[],
        &[foo(&base, &new)],
        &["--format", "json"],
    );
    let result = json(&output);
    let keys: Vec<&str> = result["changes"][0]["functions"]
        .as_array()
        .unwrap()
        .iter()
        .map(|function| return function["key"].as_str().unwrap())
        .collect();
    assert_eq!(
        keys,
        ["{closure}@app/Foo.php:27-27"],
        "helper itself did not change"
    );
    assert_eq!(result["tests"], serde_json::json!(["T\\B::two"]), "line 27");
}

#[test]
fn the_filter_names_every_selected_method() {
    let base = base_source();
    let new = edited("$label .= $item;", "$label .= $item . ',';");
    let output = impact(
        "impact_filter",
        &[],
        &[foo(&base, &new)],
        &["--format", "filter"],
    );
    assert_eq!(output.status.code(), Some(0), "{}", stderr(&output));
    assert_eq!(
        stdout(&output),
        "/^(?:T\\\\A::(?:one)(?: |$)|T\\\\B::(?:two)(?: |$))/i\n",
        "one alternative per class"
    );
}

#[test]
fn comments_outside_functions_select_nothing() {
    let base = base_source();
    let new = edited("namespace App;\n", "namespace App;\n\n// the foo\n");
    let output = impact("impact_comment", &[], &[foo(&base, &new)], &[]);
    assert_eq!(output.status.code(), Some(0), "{}", stderr(&output));
    assert!(
        stdout(&output).contains("  no function changed: only comments and whitespace\n"),
        "{}",
        stdout(&output)
    );
}

#[test]
fn an_added_method_cannot_be_bounded() {
    let base = base_source();
    let new = edited(
        "    public function baz",
        "    public function qux(): int\n    {\n        return 0;\n    }\n\n    public function baz",
    );
    let output = impact("impact_added_method", &[], &[foo(&base, &new)], &[]);
    assert_eq!(output.status.code(), Some(1), "{}", stderr(&output));
    let text = stdout(&output);
    assert!(
        text.contains("  run every test: line 19 changes declarations outside function bodies"),
        "{text}"
    );
    assert!(
        text.ends_with("impact: run every test, 1 change(s) cannot be bounded by the baseline\n"),
        "{text}"
    );
    let filter = impact(
        "impact_added_method_filter",
        &[],
        &[foo(&base, &new)],
        &["--format", "filter"],
    );
    assert_eq!(filter.status.code(), Some(1), "unbounded");
    assert_eq!(stdout(&filter), "", "no filter for an unbounded change");
    assert!(
        stderr(&filter).contains("run every test"),
        "{}",
        stderr(&filter)
    );
}

#[test]
fn non_php_changes_are_unbounded_unless_ignored() {
    let readme = Change {
        path: "README.md",
        old: Some("one\n"),
        new: Some("two\n"),
    };
    let setup = [("README.md", "one\n")];
    let output = impact("impact_readme", &setup, &[readme], &[]);
    assert_eq!(output.status.code(), Some(1), "{}", stderr(&output));
    assert!(
        stdout(&output).contains("README.md\n  run every test: not a .php file\n"),
        "{}",
        stdout(&output)
    );
    let readme = Change {
        path: "README.md",
        old: Some("one\n"),
        new: Some("two\n"),
    };
    let output = impact(
        "impact_readme_ignored",
        &setup,
        &[readme],
        &["--ignore", "README.md"],
    );
    assert_eq!(output.status.code(), Some(0), "{}", stderr(&output));
    assert!(
        stdout(&output).contains("README.md\n  ignored\n"),
        "{}",
        stdout(&output)
    );
}

#[test]
fn test_classes_run_whole_and_new_files_run_their_classes() {
    let changed = TEST_CLASS.replace(
        "    {\n    }",
        "    {\n        self::assertTrue(true);\n    }",
    );
    let new_test = TEST_CLASS.replace("class A", "class C");
    let output = impact(
        "impact_test_class",
        &[("tests/AT.php", TEST_CLASS)],
        &[
            Change {
                path: "tests/AT.php",
                old: Some(TEST_CLASS),
                new: Some(&changed),
            },
            Change {
                path: "tests/CT.php",
                old: None,
                new: Some(&new_test),
            },
        ],
        &["--format", "json"],
    );
    assert_eq!(output.status.code(), Some(0), "{}", stderr(&output));
    let result = json(&output);
    assert_eq!(
        result["classes"],
        serde_json::json!(["T\\A", "T\\C"]),
        "classes"
    );
    assert_eq!(
        result["changes"][0]["effect"], "test_classes",
        "a test class"
    );
    assert_eq!(result["changes"][1]["effect"], "new_file", "a new file");
    let filter = impact(
        "impact_test_class_filter",
        &[("tests/AT.php", TEST_CLASS)],
        &[Change {
            path: "tests/AT.php",
            old: Some(TEST_CLASS),
            new: Some(&changed),
        }],
        &["--format", "filter"],
    );
    assert_eq!(stdout(&filter), "/^(?:T\\\\A::)/i\n", "the whole class");
}

#[test]
fn a_changed_helper_outside_the_measured_code_is_unbounded() {
    let changed = HELPER.replace("return 1;", "return 2;");
    let output = impact(
        "impact_helper",
        &[("tests/Helper.php", HELPER)],
        &[Change {
            path: "tests/Helper.php",
            old: Some(HELPER),
            new: Some(&changed),
        }],
        &[],
    );
    assert_eq!(output.status.code(), Some(1), "{}", stderr(&output));
    assert!(
        stdout(&output)
            .contains("tests/Helper.php\n  run every test: neither a test class nor measured code"),
        "{}",
        stdout(&output)
    );
}

#[test]
fn a_measured_file_no_test_loaded_selects_nothing() {
    let other = HELPER.replace("namespace T;", "namespace App;");
    let changed = other.replace("return 1;", "return 2;");
    let output = impact(
        "impact_never_loaded",
        &[("app/Other.php", &other)],
        &[Change {
            path: "app/Other.php",
            old: Some(&other),
            new: Some(&changed),
        }],
        &[],
    );
    assert_eq!(output.status.code(), Some(0), "{}", stderr(&output));
    assert!(
        stdout(&output).contains("app/Other.php\n  no baseline test loaded it\n"),
        "{}",
        stdout(&output)
    );
}

#[test]
fn a_deleted_file_selects_every_test_that_ran_it() {
    let base = base_source();
    let output = impact(
        "impact_deleted",
        &[],
        &[Change {
            path: SOURCE,
            old: Some(&base),
            new: None,
        }],
        &["--format", "json"],
    );
    assert_eq!(output.status.code(), Some(0), "{}", stderr(&output));
    let result = json(&output);
    assert_eq!(result["changes"][0]["effect"], "whole_file", "deleted");
    assert_eq!(
        result["tests"],
        serde_json::json!(["T\\A::one", "T\\B::two"]),
        "tests"
    );
}

#[test]
fn a_baseline_of_another_commit_is_refused() {
    let edited_base = edited("return $x * 2;", "return $x * 4;");
    let new = edited("return $x * 2;", "return $x * 3;");
    let output = impact("impact_stale_base", &[], &[foo(&edited_base, &new)], &[]);
    assert_eq!(output.status.code(), Some(2), "{}", stdout(&output));
    assert!(
        stderr(&output).contains("was not built from the diff's base"),
        "{}",
        stderr(&output)
    );
    assert!(
        stderr(&output).contains("app/Foo.php: the diff's base has sha256"),
        "{}",
        stderr(&output)
    );
    let drifted = impact(
        "impact_stale_tree",
        &[("app/Foo.php", &new), ("README.md", "one\n")],
        &[Change {
            path: "README.md",
            old: Some("one\n"),
            new: Some("two\n"),
        }],
        &["--ignore", "README.md"],
    );
    assert_eq!(drifted.status.code(), Some(2), "{}", stdout(&drifted));
    assert!(
        stderr(&drifted).contains("app/Foo.php: the file has sha256"),
        "{}",
        stderr(&drifted)
    );
}

#[test]
fn a_diff_that_does_not_match_the_tree_is_refused() {
    let base = base_source();
    let new = edited("return $x * 2;", "return $x * 3;");
    let (directory, report) = project("impact_mismatch", Extras::default());
    fs::write(
        directory.join(SOURCE),
        edited("return $x * 2;", "return $x * 5;"),
    )
    .unwrap();
    let patch = directory.join("change.diff");
    fs::write(&patch, unified(SOURCE, Some(&base), Some(&new))).unwrap();
    let output = run(&[
        "impact",
        "--report",
        report.to_str().unwrap(),
        "--base",
        directory.to_str().unwrap(),
        "--diff",
        patch.to_str().unwrap(),
    ]);
    assert_eq!(output.status.code(), Some(2), "{}", stdout(&output));
    assert!(
        stderr(&output).contains(
            "cannot rebuild the base version of app/Foo.php from the diff: line 21 differs"
        ),
        "{}",
        stderr(&output)
    );
}

fn impact_raw(name: &str, diff: &str, prepare: impl FnOnce(&Path), extra: &[&str]) -> Output {
    let (directory, report) = project(name, Extras::default());
    prepare(&directory);
    let patch = directory.join("change.diff");
    fs::write(&patch, diff).unwrap();
    let mut arguments = vec![
        "impact",
        "--report",
        report.to_str().unwrap(),
        "--base",
        directory.to_str().unwrap(),
        "--diff",
        patch.to_str().unwrap(),
    ];
    arguments.extend_from_slice(extra);
    return run(&arguments);
}

#[test]
fn a_binary_change_is_unbounded_unless_ignored() {
    let diff = "diff --git a/docs/logo.png b/docs/logo.png\nindex 1111111..2222222 100644\nBinary files a/docs/logo.png and b/docs/logo.png differ\n";
    let output = impact_raw("impact_binary", diff, |_| {}, &[]);
    assert_eq!(output.status.code(), Some(1), "{}", stderr(&output));
    assert!(
        stdout(&output).contains("docs/logo.png\n  run every test: a binary file\n"),
        "{}",
        stdout(&output)
    );
    let ignored = impact_raw("impact_binary_ignored", diff, |_| {}, &["--ignore", "docs"]);
    assert_eq!(ignored.status.code(), Some(0), "{}", stderr(&ignored));
}

#[test]
fn a_renamed_measured_file_selects_every_test_that_ran_it() {
    let diff = "diff --git a/app/Foo.php b/app/Bar.php\nsimilarity index 100%\nrename from app/Foo.php\nrename to app/Bar.php\n";
    let output = impact_raw(
        "impact_renamed",
        diff,
        |directory| fs::rename(directory.join(SOURCE), directory.join("app/Bar.php")).unwrap(),
        &["--format", "json"],
    );
    assert_eq!(output.status.code(), Some(0), "{}", stderr(&output));
    let result = json(&output);
    assert_eq!(result["changes"][0]["old"], "app/Foo.php", "old path");
    assert_eq!(result["changes"][0]["new"], "app/Bar.php", "new path");
    assert_eq!(result["changes"][0]["effect"], "whole_file", "renamed");
    assert_eq!(
        result["tests"],
        serde_json::json!(["T\\A::one", "T\\B::two"]),
        "tests"
    );
}
