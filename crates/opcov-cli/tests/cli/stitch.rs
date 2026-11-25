use std::fs;
use std::path::{Path, PathBuf};
use std::process::Output;

use serde_json::{Value, json};

use super::fixture::{SOURCE, fixture_source, run, sha256, stderr, unified, workspace};

const BAR: &str = "app/Bar.php";
const BAR_SOURCE: &str = "<?php\n\nnamespace App;\n\nfunction bar(): int\n{\n    return 1;\n}\n";
const TESTS: [&str; 3] = ["T\\A::one", "T\\B::two", "T\\C::three"];

fn document(root: &Path, tests: &[&str], files: &Value) -> String {
    return json!({
        "schema": "opcov-report/1",
        "generator": {"opcov": "1.0.0", "php": "8.5.12", "zts": false, "vm": "HYBRID"},
        "settings": {"collect_paths": ["app"], "exclude_paths": [], "exclude_pattern": null},
        "paths": {"mode": "relative", "root": root.to_str().unwrap()},
        "tests": tests,
        "files": files,
    })
    .to_string();
}

fn entry(source: &str, lines: &Value) -> Value {
    return json!({"sha256": sha256(source.as_bytes()), "lines": lines, "functions": {}});
}

struct Project {
    directory: PathBuf,
    baseline: PathBuf,
    diff: PathBuf,
    base: String,
    new: String,
}

fn project(name: &str, from: &str, to: &str) -> Project {
    let directory = workspace(name);
    let base = fs::read_to_string(fixture_source()).unwrap();
    assert!(base.contains(from), "the fixture contains {from:?}");
    let new = base.replacen(from, to, 1);
    fs::write(directory.join(SOURCE), &new).unwrap();
    fs::write(directory.join(BAR), BAR_SOURCE).unwrap();
    let baseline = directory.join("baseline.json");
    let files = json!({
        BAR: entry(BAR_SOURCE, &json!({"7": [1, 2]})),
        SOURCE: entry(&base, &json!({"9": [0, 1], "10": [0, 1], "14": [1], "21": []})),
    });
    fs::write(&baseline, document(&directory, &TESTS, &files)).unwrap();
    let diff = directory.join("change.diff");
    fs::write(&diff, unified(SOURCE, Some(&base), Some(&new))).unwrap();
    return Project {
        directory,
        baseline,
        diff,
        base,
        new,
    };
}

impl Project {
    fn partial(&self, tests: &[&str], files: &Value) -> PathBuf {
        let partial = self.directory.join("partial.json");
        fs::write(&partial, document(&self.directory, tests, files)).unwrap();
        return partial;
    }

    fn rerun(&self) -> PathBuf {
        return self.partial(
            &TESTS[..2],
            &json!({
                BAR: entry(BAR_SOURCE, &json!({"7": [0]})),
                SOURCE: entry(&self.new, &json!({"9": [0, 1], "10": [1], "14": [], "21": []})),
            }),
        );
    }

    fn stitch(&self, partial: &Path) -> (Output, PathBuf) {
        let output = self.directory.join("stitched.json");
        let result = run(&[
            "stitch",
            "--baseline",
            self.baseline.to_str().unwrap(),
            "--partial",
            partial.to_str().unwrap(),
            "--diff",
            self.diff.to_str().unwrap(),
            "--base",
            self.directory.to_str().unwrap(),
            "-o",
            output.to_str().unwrap(),
        ]);
        return (result, output);
    }

    fn impact(&self, extra: &[&str]) -> Value {
        let mut arguments = vec![
            "impact",
            "--report",
            self.baseline.to_str().unwrap(),
            "--diff",
            self.diff.to_str().unwrap(),
            "--base",
            self.directory.to_str().unwrap(),
            "--format",
            "json",
        ];
        arguments.extend_from_slice(extra);
        let output = run(&arguments);
        assert_eq!(output.status.code(), Some(0), "{}", stderr(&output));
        return serde_json::from_slice(&output.stdout).unwrap();
    }
}

const BODY: (&str, &str) = ("$label .= $item;", "$label .= $item . ',';");

#[test]
fn whole_files_selects_every_test_that_ran_a_changed_file() {
    let project = project("stitch_whole_files", "return $x * 2;", "return $x * 3;");
    let functions = project.impact(&[]);
    assert_eq!(functions["tests"], json!([]), "nothing reached baz");
    let files = project.impact(&["--whole-files"]);
    assert_eq!(
        files["tests"],
        json!(["T\\A::one", "T\\B::two"]),
        "every test that ran app/Foo.php"
    );
    assert_eq!(
        files["changes"][0]["file_tests"],
        json!(["T\\A::one", "T\\B::two"]),
        "the change lists the tests that ran its file"
    );
    assert!(
        functions["changes"][0].get("file_tests").is_none(),
        "only --whole-files lists them"
    );
}

#[test]
fn the_rerun_replaces_its_tests_and_takes_the_changed_files() {
    let project = project("stitch_rerun", BODY.0, BODY.1);
    let (output, stitched) = project.stitch(&project.rerun());
    assert_eq!(output.status.code(), Some(0), "{}", stderr(&output));
    let report: Value = serde_json::from_slice(&fs::read(stitched).unwrap()).unwrap();
    assert_eq!(report["tests"], json!(TESTS), "every test of the suite");
    assert_eq!(
        report["files"][SOURCE],
        entry(
            &project.new,
            &json!({"9": [0, 1], "10": [1], "14": [], "21": []})
        ),
        "the changed file comes from the rerun"
    );
    assert_eq!(
        report["files"][BAR],
        entry(BAR_SOURCE, &json!({"7": [0, 2]})),
        "an unchanged file keeps the tests that did not rerun and takes the rerun's hits"
    );
    assert_ne!(project.base, project.new, "the change is real");
}

#[test]
fn a_rerun_without_a_selected_test_is_refused() {
    let project = project("stitch_missing", BODY.0, BODY.1);
    let partial = project.partial(
        &TESTS[..1],
        &json!({SOURCE: entry(&project.new, &json!({"9": [0]}))}),
    );
    let (output, stitched) = project.stitch(&partial);
    assert_eq!(output.status.code(), Some(1), "{}", stderr(&output));
    assert!(
        stderr(&output)
            .starts_with("cannot stitch: 1 test(s) that reach the change did not run in "),
        "{}",
        stderr(&output)
    );
    assert!(
        stderr(&output).contains("\n  T\\B::two"),
        "{}",
        stderr(&output)
    );
    assert!(!stitched.exists(), "nothing is written");
}

#[test]
fn an_unbounded_change_is_refused() {
    let project = project(
        "stitch_unbounded",
        "    public function baz",
        "    public const LIMIT = 1;\n\n    public function baz",
    );
    let (output, stitched) = project.stitch(&project.rerun());
    assert_eq!(output.status.code(), Some(1), "{}", stderr(&output));
    assert!(
        stderr(&output)
            .starts_with("cannot stitch: 1 change(s) reach tests the baseline cannot name"),
        "{}",
        stderr(&output)
    );
    assert!(
        stderr(&output).contains("\n  app/Foo.php: line 19 changes declarations"),
        "{}",
        stderr(&output)
    );
    assert!(!stitched.exists(), "nothing is written");
}

#[test]
fn a_partial_report_of_another_tree_is_refused() {
    let project = project("stitch_other_tree", BODY.0, BODY.1);
    let partial = project.partial(
        &TESTS[..2],
        &json!({SOURCE: entry(&project.base, &json!({"9": [0, 1]}))}),
    );
    let (output, stitched) = project.stitch(&partial);
    assert_eq!(output.status.code(), Some(2), "{}", stderr(&output));
    assert!(
        stderr(&output)
            .contains("was not built from the working tree:\n  app/Foo.php: the file has sha256"),
        "{}",
        stderr(&output)
    );
    assert!(!stitched.exists(), "nothing is written");
}

#[test]
fn a_changed_test_class_drops_its_baseline_tests() {
    let project = project("stitch_test_class", BODY.0, BODY.1);
    let old =
        "<?php\n\nnamespace T;\n\nclass A\n{\n    public function one(): void\n    {\n    }\n}\n";
    let new = old.replace("function one", "function uno");
    fs::write(project.directory.join(SOURCE), &project.base).unwrap();
    fs::create_dir_all(project.directory.join("tests")).unwrap();
    fs::write(project.directory.join("tests/A.php"), &new).unwrap();
    fs::write(&project.diff, unified("tests/A.php", Some(old), Some(&new))).unwrap();
    let partial = project.partial(
        &["T\\A::uno"],
        &json!({SOURCE: entry(&project.base, &json!({"9": [0], "10": [], "14": [], "21": []}))}),
    );
    let (output, stitched) = project.stitch(&partial);
    assert_eq!(output.status.code(), Some(0), "{}", stderr(&output));
    let report: Value = serde_json::from_slice(&fs::read(stitched).unwrap()).unwrap();
    assert_eq!(
        report["tests"],
        json!(["T\\A::uno", "T\\B::two", "T\\C::three"]),
        "the removed test is gone and the new one is in"
    );
    assert_eq!(
        report["files"][SOURCE],
        entry(
            &project.base,
            &json!({"9": [0, 1], "10": [1], "14": [1], "21": []})
        ),
        "the class's baseline hits give way to the rerun's"
    );
    assert_eq!(
        report["files"][BAR],
        entry(BAR_SOURCE, &json!({"7": [1, 2]})),
        "a file the rerun did not load keeps the other tests' hits"
    );
}
