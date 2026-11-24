use std::fs;
use std::path::{Path, PathBuf};

use roxmltree::{Document, Node, ParsingOptions};

use super::fixture::{Extras, project, report, run, stderr, with_eval_entry, workspace};

const ROOT: &str = "/srv/app";
const EVAL_SITE: &str = "Foo.php(12)";
const SHA: &str = "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";

fn golden(name: &str) -> String {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/golden")
        .join(name);
    return fs::read_to_string(&path).unwrap();
}

fn convert_report(directory: &Path, report_path: &Path, format: &str) -> String {
    let out = directory.join(format!("out.{format}"));
    let output = run(&[
        "convert",
        report_path.to_str().unwrap(),
        "--format",
        format,
        "-o",
        out.to_str().unwrap(),
    ]);
    assert_eq!(
        output.status.code(),
        Some(0),
        "convert to {format} succeeds: {}",
        stderr(&output)
    );
    return fs::read_to_string(&out).unwrap();
}

fn fixed_report(name: &str) -> (PathBuf, PathBuf) {
    let directory = workspace(name);
    let path = directory.join("report.json");
    fs::write(&path, report(ROOT, SHA, Extras::default())).unwrap();
    return (directory, path);
}

fn converted(name: &str, format: &str) -> String {
    let (directory, path) = fixed_report(name);
    let first = convert_report(&directory, &path, format);
    let second = convert_report(&directory, &path, format);
    assert_eq!(
        first, second,
        "{format} output is byte-identical across runs"
    );
    return first;
}

fn parse(xml: &str) -> Document<'_> {
    let options = ParsingOptions {
        allow_dtd: true,
        ..ParsingOptions::default()
    };
    return Document::parse_with_options(xml, options).unwrap();
}

fn elements<'a, 'i>(node: Node<'a, 'i>) -> Vec<Node<'a, 'i>> {
    return node.children().filter(Node::is_element).collect();
}

fn names(nodes: &[Node<'_, '_>]) -> Vec<String> {
    return nodes
        .iter()
        .map(|node| return node.tag_name().name().to_owned())
        .collect();
}

fn assert_attributes(node: Node<'_, '_>, required: &[&str]) {
    for attribute in required {
        assert!(
            node.attribute(*attribute).is_some(),
            "<{}> carries the required attribute {attribute}",
            node.tag_name().name()
        );
    }
}

fn assert_integer(node: Node<'_, '_>, attribute: &str) {
    let value = node.attribute(attribute).unwrap();
    assert!(
        value.parse::<u64>().is_ok(),
        "<{}> {attribute}={value:?} is an integer",
        node.tag_name().name()
    );
}

#[test]
fn lcov_matches_the_documented_mapping() {
    let lcov = converted("lcov_mapping", "lcov");
    assert_eq!(lcov, golden("report.lcov"), "lcov output");
    assert!(
        lcov.contains("BRDA:13,4,6,0\n"),
        "the unseen LoopExit outcome o6 of the third function is block 4, branch 6, taken 0"
    );
    assert!(
        lcov.contains("BRDA:13,5,2,0\nBRDA:13,5,3,0\n"),
        "loop l1 becomes block 5 with branch 2 (zero iterations) and 3 (two or more)"
    );
    assert!(
        !lcov.contains("BRDA:16,"),
        "engine outcomes do not count toward verdicts and are left out"
    );
    assert!(
        lcov.contains("FN:9,App\\Foo::bar\n"),
        "a function record sits on the function's first executable line"
    );
    assert!(
        !lcov.contains("{main}"),
        "a function without a known range gets no function record"
    );
}

#[test]
fn lcov_marks_branches_on_unexecuted_lines_as_not_reached() {
    let directory = workspace("lcov_unreached");
    let path = directory.join("report.json");
    let unexecuted = report(ROOT, SHA, Extras::default()).replace(r#""13":[1]"#, r#""13":[]"#);
    fs::write(&path, unexecuted).unwrap();
    let lcov = convert_report(&directory, &path, "lcov");
    assert!(
        lcov.contains("BRDA:13,4,6,-\n"),
        "a branch whose line never ran is `-`: {lcov}"
    );
    assert!(
        lcov.contains("BRDA:13,4,5,1\n"),
        "a branch with hits keeps its count even when its line has none: {lcov}"
    );
}

fn assert_clover_metrics(document: &Document<'_>) {
    let metrics = [
        "complexity",
        "elements",
        "coveredelements",
        "conditionals",
        "coveredconditionals",
        "statements",
        "coveredstatements",
        "coveredmethods",
        "methods",
    ];
    for node in document
        .descendants()
        .filter(|node| return node.has_tag_name("metrics"))
    {
        assert_attributes(node, &metrics);
    }
}

#[test]
fn clover_has_the_structure_of_the_php_clover_schema() {
    let clover = converted("clover_structure", "clover");
    assert_eq!(clover, golden("report.clover.xml"), "clover output");
    let document = parse(&clover);
    let coverage = document.root_element();
    assert_eq!(coverage.tag_name().name(), "coverage", "root element");
    assert_attributes(coverage, &["clover", "generated"]);
    assert_integer(coverage, "generated");
    let project = elements(coverage);
    assert_eq!(names(&project), ["project"], "coverage holds the project");
    let project = project[0];
    assert_integer(project, "timestamp");
    let children = elements(project);
    assert_eq!(
        names(&children)[..2],
        ["metrics", "package"],
        "metrics come first"
    );
    assert_clover_metrics(&document);
    let package = children[1];
    assert_eq!(
        package.attribute("name"),
        Some("App"),
        "the namespace is the package"
    );
    let files = elements(package);
    assert_eq!(
        names(&files),
        ["metrics", "file"],
        "a package holds metrics and files"
    );
    let file = files[1];
    assert_eq!(
        file.attribute("name"),
        Some("/srv/app/app/Foo.php"),
        "file names carry the path, as PHP consumers expect"
    );
    assert_eq!(
        file.attribute("path"),
        file.attribute("name"),
        "path repeats the name"
    );
    let parts = elements(file);
    assert_eq!(
        parts[0].tag_name().name(),
        "metrics",
        "file metrics come first"
    );
    assert_eq!(
        parts[1].attribute("name"),
        Some("App\\Foo"),
        "classes follow"
    );
    assert_eq!(
        parts[1].attribute("namespace"),
        Some("App"),
        "with their namespace"
    );
    let lines: Vec<Node<'_, '_>> = parts
        .iter()
        .copied()
        .filter(|node| return node.has_tag_name("line"))
        .collect();
    for line in &lines {
        assert_integer(*line, "num");
        assert!(
            matches!(line.attribute("type"), Some("method" | "stmt" | "cond")),
            "line type is a Clover construct"
        );
    }
    let condition = lines
        .iter()
        .find(|line| return line.attribute("num") == Some("13"))
        .unwrap();
    assert_eq!(
        condition.attribute("type"),
        Some("cond"),
        "line 13 has branches"
    );
    assert_eq!(
        condition.attribute("truecount"),
        Some("1"),
        "one of four taken"
    );
    assert_eq!(
        condition.attribute("falsecount"),
        Some("3"),
        "three never taken"
    );
}

#[test]
fn cobertura_has_the_structure_of_its_dtd() {
    let cobertura = converted("cobertura_structure", "cobertura");
    assert_eq!(
        cobertura,
        golden("report.cobertura.xml"),
        "cobertura output"
    );
    assert!(
        cobertura.contains(
            "<!DOCTYPE coverage SYSTEM \"http://cobertura.sourceforge.net/xml/coverage-04.dtd\">"
        ),
        "the document declares the Cobertura DTD"
    );
    let document = parse(&cobertura);
    let coverage = document.root_element();
    assert_attributes(
        coverage,
        &[
            "line-rate",
            "branch-rate",
            "lines-covered",
            "lines-valid",
            "branches-covered",
            "branches-valid",
            "complexity",
            "version",
            "timestamp",
        ],
    );
    assert_eq!(
        names(&elements(coverage)),
        ["sources", "packages"],
        "coverage children"
    );
    let rates = ["line-rate", "branch-rate", "complexity"];
    for package in document
        .descendants()
        .filter(|node| return node.has_tag_name("package"))
    {
        assert_attributes(package, &[&["name"][..], &rates[..]].concat());
        assert_eq!(names(&elements(package)), ["classes"], "package children");
    }
    for class in document
        .descendants()
        .filter(|node| return node.has_tag_name("class"))
    {
        assert_attributes(class, &[&["name", "filename"][..], &rates[..]].concat());
        assert_eq!(
            names(&elements(class)),
            ["methods", "lines"],
            "class children"
        );
    }
    for method in document
        .descendants()
        .filter(|node| return node.has_tag_name("method"))
    {
        assert_attributes(method, &[&["name", "signature"][..], &rates[..]].concat());
        assert_eq!(names(&elements(method)), ["lines"], "method children");
    }
    for line in document
        .descendants()
        .filter(|node| return node.has_tag_name("line"))
    {
        assert_attributes(line, &["number", "hits"]);
        assert_eq!(
            line.attribute("branch") == Some("true"),
            line.attribute("condition-coverage").is_some(),
            "only branch lines carry condition coverage"
        );
    }
    let loop_line = document
        .descendants()
        .find(|node| return node.has_tag_name("line") && node.attribute("number") == Some("13"))
        .unwrap();
    assert_eq!(
        loop_line.attribute("condition-coverage"),
        Some("25% (1/4)"),
        "two loop-entry outcomes and the two loop findings, one taken"
    );
    assert_eq!(
        coverage.attribute("line-rate"),
        Some("0.8888"),
        "rates are truncated to four decimals"
    );
}

#[test]
fn eval_entries_are_left_out_of_every_format() {
    let (directory, report_path) = project("convert_eval", Extras::default());
    let plain = ["lcov", "clover", "cobertura", "html"]
        .map(|format| return convert_report(&directory, &report_path, format));
    let with_eval = with_eval_entry(&fs::read_to_string(&report_path).unwrap());
    fs::write(&report_path, with_eval).unwrap();
    for (format, plain) in ["lcov", "clover", "cobertura", "html"].iter().zip(plain) {
        let converted = convert_report(&directory, &report_path, format);
        assert!(
            !converted.contains(EVAL_SITE),
            "{format} names no eval entry: {converted}"
        );
        assert_eq!(
            converted, plain,
            "{format} output is the same as without the eval entry"
        );
    }
}

#[test]
fn xml_escapes_paths() {
    let directory = workspace("xml_escapes");
    let path = directory.join("report.json");
    let odd = report(ROOT, SHA, Extras::default()).replace("app/Foo.php", "app/A&B<'>.php");
    fs::write(&path, odd).unwrap();
    for format in ["clover", "cobertura"] {
        let xml = convert_report(&directory, &path, format);
        let document = parse(&xml);
        let named = document.descendants().any(|node| {
            return [node.attribute("name"), node.attribute("filename")]
                .contains(&Some("app/A&B<'>.php"))
                || node.attribute("name") == Some("/srv/app/app/A&B<'>.php");
        });
        assert!(named, "{format} round-trips the escaped path: {xml}");
    }
}

#[test]
fn html_shows_every_source_line_with_its_coverage() {
    let (directory, report_path) = project("html_report", Extras::default());
    let html = convert_report(&directory, &report_path, "html");
    assert!(html.starts_with("<!DOCTYPE html>"), "an HTML document");
    assert!(
        html.contains("<td class=\"s\">&lt;?php</td>"),
        "source is escaped"
    );
    assert!(
        html.contains("<tr class=\"uncovered\"><td class=\"n\">21</td><td class=\"t\">0</td>"),
        "the uncovered line 21 is marked"
    );
    assert!(
        html.contains("<tr class=\"partial\"><td class=\"n\">13</td><td class=\"t\">1</td><td class=\"b\">1/4</td>"),
        "line 13 ran but took one of four branches"
    );
    assert!(
        html.contains("<td class=\"n\">29</td>"),
        "every source line is listed"
    );
}

#[test]
fn html_refuses_a_changed_source_with_exit_code_two() {
    let (directory, report_path) = project("html_stale", Extras::default());
    fs::write(directory.join("app/Foo.php"), "<?php\n").unwrap();
    let out = directory.join("out.html");
    let output = run(&[
        "convert",
        report_path.to_str().unwrap(),
        "--format",
        "html",
        "-o",
        out.to_str().unwrap(),
    ]);
    assert_eq!(
        output.status.code(),
        Some(2),
        "a stale source cannot be rendered"
    );
    assert!(
        stderr(&output).contains("changed since the report was written"),
        "{}",
        stderr(&output)
    );
    assert!(!out.exists(), "nothing is written");
}

#[test]
fn convert_reports_unreadable_input_and_unknown_formats_with_exit_code_two() {
    let directory = workspace("convert_errors");
    let missing = directory.join("missing.json");
    let out = directory.join("out.lcov");
    let output = run(&[
        "convert",
        missing.to_str().unwrap(),
        "--format",
        "lcov",
        "-o",
        out.to_str().unwrap(),
    ]);
    assert_eq!(
        output.status.code(),
        Some(2),
        "a missing report is an error"
    );
    assert!(stderr(&output).contains("missing.json"), "names the input");
    let (_, path) = fixed_report("convert_unknown_format");
    let output = run(&[
        "convert",
        path.to_str().unwrap(),
        "--format",
        "cov",
        "-o",
        out.to_str().unwrap(),
    ]);
    assert_eq!(
        output.status.code(),
        Some(2),
        "an unknown format is a usage error"
    );
}
