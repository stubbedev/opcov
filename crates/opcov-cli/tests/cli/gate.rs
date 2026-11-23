use std::fs;
use std::io::Write;
use std::path::Path;
use std::process::{Command, Stdio};

use serde_json::Value;

use super::fixture::{BINARY, Extras, project, run, stderr, stdout};

fn gate(directory: &Path, report: &Path, arguments: &[&str]) -> std::process::Output {
    let mut all = vec![
        "gate",
        "--report",
        report.to_str().unwrap(),
        "--base",
        directory.to_str().unwrap(),
    ];
    all.extend_from_slice(arguments);
    return run(&all);
}

fn json(output: &std::process::Output) -> Value {
    return serde_json::from_str(&stdout(output)).unwrap();
}

fn function_keys(result: &Value) -> Vec<String> {
    return result["files"]
        .as_array()
        .unwrap()
        .iter()
        .flat_map(|file| return file["functions"].as_array().unwrap().iter())
        .map(|function| return function["key"].as_str().unwrap().to_owned())
        .collect();
}

fn function<'v>(result: &'v Value, key: &str) -> &'v Value {
    return result["files"][0]["functions"]
        .as_array()
        .unwrap()
        .iter()
        .find(|function| return function["key"] == key)
        .unwrap();
}

#[test]
fn reports_uncovered_lines_of_a_changed_function() {
    let (directory, report) = project("gate_uncovered", Extras::default());
    let output = gate(&directory, &report, &["--files", "app/Foo.php:21"]);
    assert_eq!(
        output.status.code(),
        Some(1),
        "uncovered lines are findings"
    );
    let text = stdout(&output);
    assert!(text.contains("  App\\Foo::baz (lines 19-22)\n"), "{text}");
    assert!(text.contains("    uncovered lines: 21\n"), "{text}");
    assert!(
        text.contains("gate: 1 finding(s) in 1 changed function(s)"),
        "{text}"
    );
}

#[test]
fn reports_unseen_outcomes_and_loop_findings_with_the_tests_per_line() {
    let (directory, report) = project("gate_outcomes", Extras::default());
    let output = gate(
        &directory,
        &report,
        &["--files", "app/Foo.php:14", "--format", "json"],
    );
    assert_eq!(
        output.status.code(),
        Some(1),
        "unseen outcomes are findings"
    );
    let result = json(&output);
    assert_eq!(result["verdict"], "findings", "verdict");
    assert_eq!(
        function_keys(&result),
        ["App\\Foo::bar"],
        "only bar changed"
    );
    let bar = function(&result, "App\\Foo::bar");
    assert_eq!(
        bar["span"],
        serde_json::json!([7, 5, 17, 6]),
        "span from the source"
    );
    assert_eq!(
        bar["uncovered_lines"],
        serde_json::json!([]),
        "every line ran"
    );
    assert_eq!(
        bar["unseen_outcomes"],
        serde_json::json!([{"id": "o6", "kind": "LoopExit", "line": 13, "span": null}]),
        "the engine outcome o7 does not count toward verdicts"
    );
    let looped = serde_json::json!([{"id": "l1", "kind": "Foreach", "line": 13}]);
    assert_eq!(
        bar["loops_never_zero"], looped,
        "the loop never ran zero times"
    );
    assert_eq!(bar["loops_never_twice"], looped, "the loop never ran twice");
    assert_eq!(
        bar["lines"][0],
        serde_json::json!({"line": 9, "tests": ["T\\A::one", "T\\B::two"]}),
        "each line lists the tests that ran it"
    );
    assert_eq!(result["findings"], 3, "three findings");
}

#[test]
fn passes_a_covered_function_and_checks_its_closures() {
    let (directory, report) = project("gate_pass", Extras::default());
    let output = gate(
        &directory,
        &report,
        &["--files", "app/Foo.php:28", "--format", "json"],
    );
    assert_eq!(output.status.code(), Some(0), "{}", stdout(&output));
    let result = json(&output);
    assert_eq!(result["verdict"], "pass", "verdict");
    assert_eq!(
        function_keys(&result),
        ["App\\helper", "{closure}@app/Foo.php:27-27"],
        "the arrow function inside the changed function is checked too"
    );
}

#[test]
fn a_change_inside_a_closure_checks_only_the_closure() {
    let (directory, report) = project("gate_closure", Extras::default());
    let output = gate(
        &directory,
        &report,
        &["--files", "app/Foo.php:27", "--format", "json"],
    );
    assert_eq!(output.status.code(), Some(0), "{}", stdout(&output));
    assert_eq!(
        function_keys(&json(&output)),
        ["{closure}@app/Foo.php:27-27"],
        "the innermost function owns the line"
    );
}

#[test]
fn a_bare_path_checks_every_function_of_the_file() {
    let (directory, report) = project("gate_whole", Extras::default());
    let output = gate(
        &directory,
        &report,
        &["--files", "app/Foo.php", "--format", "json"],
    );
    assert_eq!(output.status.code(), Some(1), "baz and bar have findings");
    assert_eq!(
        function_keys(&json(&output)),
        [
            "{main}@app/Foo.php",
            "App\\Foo::bar",
            "App\\Foo::baz",
            "App\\helper",
            "{closure}@app/Foo.php:27-27"
        ],
        "every function, in source order"
    );
}

fn edit_report(report: &Path, from: &str, to: &str) {
    let text = fs::read_to_string(report).unwrap();
    assert!(text.contains(from), "the report holds {from}");
    fs::write(report, text.replacen(from, to, 1)).unwrap();
}

fn write_source(directory: &Path, relative: &str) {
    let path = directory.join(relative);
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(
        path,
        "<?php\n\nfunction fresh(): int\n{\n    return 1;\n}\n",
    )
    .unwrap();
}

#[test]
fn a_changed_file_no_test_loaded_is_a_finding() {
    let (directory, report) = project("gate_unloaded", Extras::default());
    write_source(&directory, "app/New.php");
    let output = gate(&directory, &report, &["--files", "app/New.php:3-5,1"]);
    assert_eq!(
        output.status.code(),
        Some(1),
        "a selected file that no test loaded fails the gate"
    );
    let text = stdout(&output);
    assert!(
        text.contains("app/New.php\n  never loaded by any test: changed lines 1, 3-5\n"),
        "{text}"
    );
    assert!(
        text.contains(
            "gate: 1 finding(s) in 0 changed function(s) and 1 changed file(s) no test loaded\n"
        ),
        "{text}"
    );
    let output = gate(
        &directory,
        &report,
        &[
            "--files",
            "app/New.php",
            "app/Foo.php:28",
            "--format",
            "json",
        ],
    );
    assert_eq!(output.status.code(), Some(1), "{}", stdout(&output));
    let result = json(&output);
    assert_eq!(result["verdict"], "findings", "verdict");
    assert_eq!(result["findings"], 1, "one finding per unloaded file");
    assert_eq!(
        result["unloaded"],
        serde_json::json!([{"path": "app/New.php", "changed": "whole_file"}]),
        "a bare path changes the whole file"
    );
    assert_eq!(
        function_keys(&result),
        ["App\\helper", "{closure}@app/Foo.php:27-27"],
        "loaded files are still checked per function"
    );
    assert_eq!(result["unmeasured"], serde_json::json!([]), "unmeasured");
}

#[test]
fn a_diff_of_a_file_no_test_loaded_lists_its_changed_and_removed_lines() {
    let (directory, report) = project("gate_unloaded_diff", Extras::default());
    write_source(&directory, "app/New.php");
    let patch = directory.join("change.diff");
    fs::write(
        &patch,
        "--- a/app/New.php\n+++ b/app/New.php\n@@ -5 +5 @@\n-    return 2;\n+    return 1;\n@@ -7,2 +6,0 @@\n-\n-function gone() {}\n",
    )
    .unwrap();
    let output = gate(
        &directory,
        &report,
        &["--diff", patch.to_str().unwrap(), "--format", "json"],
    );
    assert_eq!(output.status.code(), Some(1), "{}", stdout(&output));
    assert_eq!(
        json(&output)["unloaded"],
        serde_json::json!([{
            "path": "app/New.php",
            "changed": {"parts": {"lines": [5], "removed_before": [7]}}
        }]),
        "changed lines and the gaps of removed lines"
    );
}

#[test]
fn lists_changed_files_that_cannot_be_measured_without_failing() {
    let (directory, report) = project("gate_unmeasured", Extras::default());
    write_source(&directory, "tests/FooTest.php");
    write_source(&directory, "app/Generated/Proxy.php");
    edit_report(
        &report,
        r#""exclude_paths":[]"#,
        r#""exclude_paths":["app/Generated"]"#,
    );
    let output = gate(
        &directory,
        &report,
        &[
            "--files",
            "README.md",
            "../outside.php",
            "app/Gone.php",
            "tests/FooTest.php",
            "app/Generated/Proxy.php",
        ],
    );
    assert_eq!(
        output.status.code(),
        Some(0),
        "unmeasurable files are not findings"
    );
    let text = stdout(&output);
    for line in [
        "not in the report: README.md (not a .php file)\n",
        "not in the report: ../outside.php (outside the report's root)\n",
        "not in the report: app/Gone.php (does not exist)\n",
        "not in the report: tests/FooTest.php (not in opcov.collect_paths)\n",
        "not in the report: app/Generated/Proxy.php (excluded by opcov.exclude_paths entry ",
        "gate: pass, 0 changed function(s) checked",
    ] {
        assert!(text.contains(line), "{line:?} in {text}");
    }
}

#[test]
fn does_not_guess_what_an_exclude_pattern_selects() {
    let (directory, report) = project("gate_pattern", Extras::default());
    write_source(&directory, "app/New.php");
    edit_report(
        &report,
        r#""exclude_pattern":null"#,
        r#""exclude_pattern":"~/New~""#,
    );
    let output = gate(
        &directory,
        &report,
        &["--files", "app/New.php", "--format", "json"],
    );
    assert_eq!(output.status.code(), Some(0), "{}", stdout(&output));
    let result = json(&output);
    assert_eq!(result["unloaded"], serde_json::json!([]), "no finding");
    assert_eq!(
        result["unmeasured"],
        serde_json::json!([{
            "path": "app/New.php",
            "reason": "the report's opcov.exclude_pattern may exclude it, and the gate cannot evaluate PCRE patterns"
        }]),
        "the pattern is named as the reason"
    );
}

#[test]
fn refuses_a_collect_path_that_no_longer_resolves() {
    let (directory, report) = project("gate_collect_missing", Extras::default());
    write_source(&directory, "app/New.php");
    edit_report(
        &report,
        r#""collect_paths":["app"]"#,
        r#""collect_paths":["src"]"#,
    );
    let output = gate(&directory, &report, &["--files", "app/New.php"]);
    assert_eq!(
        output.status.code(),
        Some(2),
        "the selection cannot be rebuilt"
    );
    let message = stderr(&output);
    assert!(
        message.contains("cannot rebuild the report's file selection relative to"),
        "{message}"
    );
    assert!(message.contains("src"), "{message}");
}

#[test]
fn reads_entries_from_list_files_and_standard_input() {
    let (directory, report) = project("gate_lists", Extras::default());
    let list = directory.join("changed.txt");
    fs::write(&list, "app/Foo.php:21\n\nREADME.md\n").unwrap();
    let mut child = Command::new(BINARY)
        .args([
            "gate",
            "--report",
            report.to_str().unwrap(),
            "--base",
            directory.to_str().unwrap(),
            "--format",
            "json",
            "--files",
            &format!("@{}", list.display()),
            "@-",
        ])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .unwrap();
    child
        .stdin
        .take()
        .unwrap()
        .write_all(b"app/Foo.php:9-10,28\n")
        .unwrap();
    let output = child.wait_with_output().unwrap();
    assert_eq!(output.status.code(), Some(1), "baz is uncovered");
    let result = json(&output);
    assert_eq!(
        function_keys(&result),
        [
            "App\\Foo::bar",
            "App\\Foo::baz",
            "App\\helper",
            "{closure}@app/Foo.php:27-27"
        ],
        "entries from both lists are united"
    );
    assert_eq!(
        result["unmeasured"],
        serde_json::json!([{"path": "README.md", "reason": "not a .php file"}]),
        "unmeasured"
    );
}

#[test]
fn refuses_a_source_that_changed_since_the_report_with_exit_code_two() {
    let (directory, report) = project("gate_stale", Extras::default());
    fs::write(directory.join("app/Foo.php"), "<?php\n// edited\n").unwrap();
    let output = gate(&directory, &report, &["--files", "app/Foo.php:21"]);
    assert_eq!(
        output.status.code(),
        Some(2),
        "a hash mismatch cannot be gated"
    );
    let message = stderr(&output);
    assert!(
        message.contains("changed since the report was written"),
        "{message}"
    );
    assert!(message.contains("app/Foo.php"), "{message}");
}

#[test]
fn reports_a_missing_source_with_exit_code_two() {
    let (directory, report) = project("gate_missing_source", Extras::default());
    fs::remove_file(directory.join("app/Foo.php")).unwrap();
    let output = gate(&directory, &report, &["--files", "app/Foo.php:21"]);
    assert_eq!(
        output.status.code(),
        Some(2),
        "a missing source cannot be gated"
    );
    assert!(
        stderr(&output).contains("cannot read source"),
        "{}",
        stderr(&output)
    );
}

#[test]
fn rejects_bad_input_with_exit_code_two() {
    let (directory, report) = project("gate_bad_input", Extras::default());
    let output = gate(&directory, &report, &["--files", "app/Foo.php:5-3"]);
    assert_eq!(
        output.status.code(),
        Some(2),
        "an inverted range is invalid"
    );
    assert!(
        stderr(&output).contains("\"5-3\" is not a line range"),
        "{}",
        stderr(&output)
    );
    let output = gate(&directory, &report, &["--files", "a", "--diff", "x.diff"]);
    assert_eq!(
        output.status.code(),
        Some(2),
        "--files and --diff exclude each other"
    );
    let output = gate(&directory, &report, &[]);
    assert_eq!(output.status.code(), Some(2), "a change set is required");
    let missing = directory.join("missing.json");
    let output = gate(&directory, &missing, &["--files", "app/Foo.php"]);
    assert_eq!(
        output.status.code(),
        Some(2),
        "an unreadable report is an error"
    );
}
