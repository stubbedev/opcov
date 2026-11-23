use std::fs;
use std::path::{Path, PathBuf};

use super::fixture::{run, sha256, stderr, stdout, workspace};

const FILES: usize = 240;
const NAMESPACES: usize = 3;

fn path_of(index: usize) -> String {
    return format!("app/F{index:04}.php");
}

fn source_of(index: usize) -> String {
    return format!(
        "<?php\nnamespace Ns{};\nclass C{index}\n{{\n    public function run(): int\n    {{\n        return 1;\n    }}\n}}\n",
        index % NAMESPACES
    );
}

fn entry(index: usize, sha: &str, hit: &str) -> String {
    let path = path_of(index);
    let namespace = index % NAMESPACES;
    return format!(
        r#""{path}":{{"sha256":"{sha}","lines":{{"7":{hit}}},"functions":{{"Ns{namespace}\\C{index}::run":{{"span":[5,5,8,6],"outcomes":[],"loops":[]}}}}}}"#
    );
}

fn many(name: &str, broken: Option<usize>) -> (PathBuf, PathBuf) {
    let directory = workspace(name);
    let mut entries = Vec::with_capacity(FILES);
    for index in 0..FILES {
        let source = source_of(index);
        fs::write(directory.join(path_of(index)), &source).unwrap();
        let hit = if broken == Some(index) {
            "[5]"
        } else if index % 2 == 0 {
            "[0]"
        } else {
            "[]"
        };
        entries.push(entry(index, &sha256(source.as_bytes()), hit));
    }
    let report = format!(
        concat!(
            r#"{{"schema":"opcov-report/1","generator":{{"opcov":"1.0.0","php":"8.5.12","zts":false,"vm":"HYBRID"}},"#,
            r#""settings":{{"collect_paths":["app"],"exclude_paths":[],"exclude_pattern":null}},"#,
            r#""paths":{{"mode":"relative","root":"{root}"}},"tests":["T::one"],"files":{{{files}}}}}"#,
            "\n"
        ),
        root = directory.to_str().unwrap(),
        files = entries.join(",")
    );
    let report_path = directory.join("report.json");
    fs::write(&report_path, report).unwrap();
    return (directory, report_path);
}

fn convert(report: &Path, format: &str, out: &Path) -> std::process::Output {
    return run(&[
        "convert",
        report.to_str().unwrap(),
        "--format",
        format,
        "-o",
        out.to_str().unwrap(),
    ]);
}

#[test]
fn lcov_lists_files_in_path_order_whatever_the_worker_count() {
    let (directory, report) = many("many-lcov", None);
    let out = directory.join("out.lcov");
    let output = convert(&report, "lcov", &out);
    assert_eq!(
        output.status.code(),
        Some(0),
        "converts: {}",
        stderr(&output)
    );
    let first = fs::read_to_string(&out).unwrap();
    let files: Vec<String> = first
        .lines()
        .filter_map(|line| return line.strip_prefix("SF:"))
        .map(|location| return location.rsplit('/').next().unwrap().to_owned())
        .collect();
    let expected: Vec<String> = (0..FILES)
        .map(|index| return path_of(index).replace("app/", ""))
        .collect();
    assert_eq!(files, expected, "one record per file, in path order");
    assert_eq!(
        convert(&report, "lcov", &out).status.code(),
        Some(0),
        "converts again"
    );
    assert_eq!(
        fs::read_to_string(&out).unwrap(),
        first,
        "the output is byte-identical"
    );
}

#[test]
fn clover_groups_interleaved_namespaces_into_packages_in_path_order() {
    let (directory, report) = many("many-clover", None);
    let out = directory.join("out.xml");
    let output = convert(&report, "clover", &out);
    assert_eq!(
        output.status.code(),
        Some(0),
        "converts: {}",
        stderr(&output)
    );
    let xml = fs::read_to_string(&out).unwrap();
    let document = roxmltree::Document::parse(&xml).unwrap();
    let packages: Vec<_> = document
        .descendants()
        .filter(|node| return node.has_tag_name("package"))
        .collect();
    assert_eq!(packages.len(), NAMESPACES, "one package per namespace");
    for (namespace, package) in packages.iter().enumerate() {
        assert_eq!(
            package.attribute("name"),
            Some(format!("Ns{namespace}").as_str()),
            "packages are in name order"
        );
        let files: Vec<String> = package
            .children()
            .filter(|node| return node.has_tag_name("file"))
            .map(|file| {
                return file
                    .attribute("name")
                    .unwrap()
                    .rsplit('/')
                    .next()
                    .unwrap()
                    .to_owned();
            })
            .collect();
        let expected: Vec<String> = (0..FILES)
            .filter(|index| return index % NAMESPACES == namespace)
            .map(|index| return path_of(index).replace("app/", ""))
            .collect();
        assert_eq!(
            files, expected,
            "package Ns{namespace} holds its files in path order"
        );
    }
    assert!(
        xml.contains("\n      <file "),
        "files inside a package are indented one level deeper"
    );
}

#[test]
fn an_invalid_entry_fails_the_conversion_naming_it_and_writes_nothing() {
    let (directory, report) = many("many-broken", Some(150));
    for format in ["lcov", "clover", "cobertura", "html"] {
        let out = directory.join(format!("out.{format}"));
        let output = convert(&report, format, &out);
        assert_eq!(output.status.code(), Some(2), "{format} fails");
        let message = stderr(&output);
        assert!(
            message.contains(&path_of(150)),
            "{format}: {message} names the entry"
        );
        assert!(!out.exists(), "{format}: no output is left behind");
    }
}

#[test]
fn doctor_and_html_read_every_source_in_parallel() {
    let (directory, report) = many("many-doctor", None);
    let output = run(&["doctor", "--report", report.to_str().unwrap()]);
    assert_eq!(
        output.status.code(),
        Some(0),
        "doctor is clean: {}",
        stderr(&output)
    );
    assert_eq!(
        stdout(&output),
        "doctor: 0 of 0 outcome(s) have no source construct\n",
        "nothing to report"
    );
    let out = directory.join("out.html");
    assert_eq!(
        convert(&report, "html", &out).status.code(),
        Some(0),
        "html converts"
    );
    let html = fs::read_to_string(&out).unwrap();
    let last = format!("<section id=\"f{}\">", FILES - 1);
    assert!(html.contains(&last), "every file has a section");
    fs::write(directory.join(path_of(120)), "<?php\n// changed\n").unwrap();
    let output = run(&["doctor", "--report", report.to_str().unwrap()]);
    assert_eq!(output.status.code(), Some(2), "a stale source stops doctor");
    assert!(
        stderr(&output).contains(&path_of(120)),
        "the stale file is named: {}",
        stderr(&output)
    );
}
