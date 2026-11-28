use std::fs;
use std::path::{Path, PathBuf};
use std::process::Output;

use serde_json::Value;

use super::fixture::{run, stderr, stdout, unified, workspace};

const REPORT: &str = "app/Services/Report.php";
const RECEIVERS: &str = "app/Uses/Receivers.php";
const LOOKALIKE: &str = "app/Uses/Lookalike.php";
const CALENDAR: &str = "app/Services/Calendar.php";
const CALENDAR_SOURCE: &str = "<?php\n\nnamespace App\\Services;\n\nclass Calendar\n{\n    public function today(Report $report): string\n    {\n        return $report->summary();\n    }\n}\n";

fn copy(from: &Path, to: &Path) {
    fs::create_dir_all(to).unwrap();
    for entry in fs::read_dir(from).unwrap() {
        let entry = entry.unwrap();
        let target = to.join(entry.file_name());
        if entry.file_type().unwrap().is_dir() {
            copy(&entry.path(), &target);
        } else {
            fs::copy(entry.path(), target).unwrap();
        }
    }
}

fn php_project(name: &str) -> PathBuf {
    let root = workspace(name);
    copy(
        &Path::new(env!("CARGO_MANIFEST_DIR")).join("../opcov-php/tests/fixtures/project"),
        &root,
    );
    return root;
}

fn replaced(source: &str, from: &str, to: &str) -> String {
    assert!(source.contains(from), "the source holds {from:?}");
    return source.replacen(from, to, 1);
}

fn change(root: &Path, path: &str, new: Option<&str>) -> String {
    let target = root.join(path);
    let old = fs::read_to_string(&target).ok();
    match new {
        Some(new) => fs::write(&target, new).unwrap(),
        None => fs::remove_file(&target).unwrap(),
    }
    return unified(path, old.as_deref(), new);
}

fn branch(root: &Path) -> PathBuf {
    let report = fs::read_to_string(root.join(REPORT)).unwrap();
    let report = replaced(&report, "    public const LIMIT = 10;\n\n", "");
    let report = replaced(
        &report,
        "    public function render(): string\n    {\n        return '';\n    }\n}\n",
        "    public function render(): string\n    {\n        return $this->summary();\n    }\n\n    public function summary(): string\n    {\n        return 'summary';\n    }\n}\n",
    );
    let receivers = replaced(
        &fs::read_to_string(root.join(RECEIVERS)).unwrap(),
        "echo Report::LIMIT, $report->title;",
        "echo $report->summary(), $report->title;",
    );
    let diff = [
        change(root, REPORT, Some(&report)),
        change(root, RECEIVERS, Some(&receivers)),
        change(root, LOOKALIKE, None),
        change(root, CALENDAR, Some(CALENDAR_SOURCE)),
    ]
    .concat();
    let patch = root.join("branch.diff");
    fs::write(&patch, diff).unwrap();
    return patch;
}

fn graph(root: &Path, patch: &Path) -> Output {
    return run(&[
        "graph",
        "--root",
        root.to_str().unwrap(),
        "--diff",
        patch.to_str().unwrap(),
    ]);
}

fn json(output: &Output) -> Value {
    assert!(output.status.success(), "{}", stderr(output));
    return serde_json::from_str(&stdout(output)).unwrap();
}

fn place(location: &Value) -> String {
    if location.is_null() {
        return "-".to_owned();
    }
    return format!("{}-{}", location["first"], location["last"]);
}

fn lines(sites: &Value) -> String {
    return sites
        .as_array()
        .unwrap()
        .iter()
        .map(|site| return site["line"].to_string())
        .collect::<Vec<_>>()
        .join(",");
}

fn described(graph: &Value) -> Vec<String> {
    let nodes = graph["nodes"].as_array().unwrap().iter().map(|node| {
        return format!(
            "{} {} {} | {} | {}",
            node["depth"],
            node["change"].as_str().unwrap(),
            node["id"].as_str().unwrap(),
            place(&node["base"]),
            place(&node["head"]),
        );
    });
    let edges = graph["edges"].as_array().unwrap().iter().map(|edge| {
        return format!(
            "{} {} -> {} | {} | {}",
            edge["change"].as_str().unwrap(),
            edge["from"].as_str().unwrap(),
            edge["to"].as_str().unwrap(),
            lines(&edge["base"]),
            lines(&edge["head"]),
        );
    });
    return nodes.chain(edges).collect();
}

#[test]
fn a_branch_is_described_from_both_sides_of_its_diff() {
    let root = php_project("graph-branch");
    let patch = branch(&root);
    assert_eq!(
        described(&json(&graph(&root, &patch))),
        [
            "0 added method App\\Services\\Calendar::today | - | 7-10",
            "0 modified method App\\Services\\Report::__construct | 16-18 | 14-16",
            "0 modified method App\\Services\\Report::render | 20-23 | 18-21",
            "0 added method App\\Services\\Report::summary | - | 23-26",
            "0 removed method App\\Uses\\Lookalike::run | 9-14 | -",
            "0 modified method App\\Uses\\Receivers::run | 13-24 | 13-24",
            "0 removed constant App\\Services\\Report::LIMIT | 12-12 | -",
            "0 modified property App\\Services\\Report::title | 14-14 | 12-12",
            "added method App\\Services\\Calendar::today -> method App\\Services\\Report::summary |  | 9",
            "added method App\\Services\\Report::render -> method App\\Services\\Report::summary |  | 20",
            "kept method App\\Uses\\Receivers::run -> method App\\Services\\Report::__construct | 22 | 22",
            "kept method App\\Uses\\Receivers::run -> method App\\Services\\Report::render | 15,16,17,19,21 | 15,16,17,19,21",
            "added method App\\Uses\\Receivers::run -> method App\\Services\\Report::summary |  | 23",
            "removed method App\\Uses\\Receivers::run -> constant App\\Services\\Report::LIMIT | 23 | ",
            "kept method App\\Uses\\Receivers::run -> property App\\Services\\Report::title | 23 | 23",
        ],
        "the diff's one hunk in Report.php replaces lines 12-23, so title and __construct are modified and move; \
         the deleted Lookalike.php removes its method, the created Calendar.php adds its own, and the removed LIMIT keeps its base caller"
    );
}

#[test]
fn the_base_index_is_not_written_to_the_projects_cache() {
    let root = php_project("graph-cache");
    let patch = branch(&root);
    json(&graph(&root, &patch));
    let cache = root.join(".opcov/index.cache");
    let written = fs::read(&cache).unwrap();
    let consumers = run(&[
        "consumers",
        "--root",
        root.to_str().unwrap(),
        "--target",
        "method App\\Services\\Report::render",
        "--no-serve",
    ]);
    assert!(consumers.status.success(), "{}", stderr(&consumers));
    assert_eq!(
        fs::read(&cache).unwrap(),
        written,
        "the cache holds the head's scans, so a search of the head reuses it unchanged"
    );
}

#[test]
fn a_working_tree_that_is_not_the_diffs_head_fails() {
    let root = php_project("graph-stale");
    let patch = branch(&root);
    fs::write(root.join(REPORT), "<?php\n").unwrap();
    let output = graph(&root, &patch);
    assert!(!output.status.success(), "{}", stdout(&output));
    assert!(
        stderr(&output)
            .contains("cannot rebuild the base version of app/Services/Report.php from the diff"),
        "{}",
        stderr(&output)
    );
}

#[test]
fn a_binary_php_change_fails() {
    let root = php_project("graph-binary");
    let patch = root.join("binary.diff");
    fs::write(
        &patch,
        "diff --git a/app/Services/Clock.php b/app/Services/Clock.php\nindex 1111111..2222222 100644\nBinary files a/app/Services/Clock.php and b/app/Services/Clock.php differ\n",
    )
    .unwrap();
    let output = graph(&root, &patch);
    assert!(!output.status.success(), "{}", stdout(&output));
    assert!(
        stderr(&output).contains("cannot rebuild the base version of app/Services/Clock.php: the diff shows a binary change"),
        "{}",
        stderr(&output)
    );
}
