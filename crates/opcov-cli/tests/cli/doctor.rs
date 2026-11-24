use std::fs;

use serde_json::Value;

use super::fixture::{
    Extras, UNKNOWN_FUNCTION, UNMATCHED_OUTCOME, project, report, run, sha256, stderr, stdout,
    with_eval_entry,
};

fn doctor(report_path: &std::path::Path, format: &str) -> std::process::Output {
    return run(&[
        "doctor",
        "--report",
        report_path.to_str().unwrap(),
        "--format",
        format,
    ]);
}

#[test]
fn lists_outcomes_without_a_source_construct() {
    let extras = Extras {
        outcomes: UNMATCHED_OUTCOME,
        ..Extras::default()
    };
    let (_, report_path) = project("doctor_unmatched", extras);
    let output = doctor(&report_path, "text");
    assert_eq!(
        output.status.code(),
        Some(1),
        "an unmatched outcome is a finding"
    );
    let text = stdout(&output);
    assert!(
        text.contains("App\\Foo::bar o8 IfTrue on line 14: no source construct of this kind can produce an opcode on this line"),
        "{text}"
    );
    assert!(
        text.contains("App\\Foo::bar o7 Engine on line 16 (excluded from verdicts): the outcome is engine-internal"),
        "{text}"
    );
    assert!(
        text.contains("doctor: 2 of 8 outcome(s) have no source construct"),
        "{text}"
    );
}

#[test]
fn passes_when_only_excluded_outcomes_are_unmatched() {
    let (_, report_path) = project("doctor_clean", Extras::default());
    let output = doctor(&report_path, "json");
    assert_eq!(output.status.code(), Some(0), "{}", stdout(&output));
    let result: Value = serde_json::from_str(&stdout(&output)).unwrap();
    assert_eq!(result["outcomes"], 7, "outcomes counted");
    assert_eq!(result["unmatched"], 1, "only the engine outcome");
    let unmatched = &result["files"][0]["unmatched_outcomes"][0];
    assert_eq!(unmatched["id"], "o7", "id");
    assert_eq!(unmatched["counts_toward_verdict"], false, "excluded");
}

#[test]
fn counts_eval_entries_instead_of_reading_a_source_for_them() {
    let (_, report_path) = project("doctor_eval", Extras::default());
    let with_eval = with_eval_entry(&fs::read_to_string(&report_path).unwrap());
    fs::write(&report_path, with_eval).unwrap();
    let output = doctor(&report_path, "json");
    assert_eq!(output.status.code(), Some(0), "{}", stderr(&output));
    let result: Value = serde_json::from_str(&stdout(&output)).unwrap();
    assert_eq!(
        result["unchecked_eval_entries"], 1,
        "the eval entry is counted"
    );
    assert_eq!(result["outcomes"], 7, "only the source file's outcomes");
    let text = stdout(&doctor(&report_path, "text"));
    assert!(
        text.contains("doctor: 1 eval()'d code entry not checked"),
        "{text}"
    );
}

#[test]
fn reports_functions_the_source_does_not_declare() {
    let extras = Extras {
        functions: UNKNOWN_FUNCTION,
        ..Extras::default()
    };
    let (_, report_path) = project("doctor_missing_function", extras);
    let output = doctor(&report_path, "text");
    assert_eq!(
        output.status.code(),
        Some(1),
        "a missing function is a finding"
    );
    assert!(
        stdout(&output)
            .contains("App\\Foo::gone: the source declares no function `App\\Foo::gone`"),
        "{}",
        stdout(&output)
    );
}

#[test]
fn fails_on_unparsable_or_stale_sources_with_exit_code_two() {
    let (directory, report_path) = project("doctor_parse_error", Extras::default());
    let broken = b"<?php\nfunction (\n";
    fs::write(directory.join("app/Foo.php"), broken).unwrap();
    let root = directory.to_str().unwrap();
    fs::write(
        &report_path,
        report(root, &sha256(broken), Extras::default()),
    )
    .unwrap();
    let output = doctor(&report_path, "text");
    assert_eq!(
        output.status.code(),
        Some(2),
        "a parse failure is a hard error"
    );
    assert!(
        stderr(&output).contains("cannot parse `app/Foo.php`"),
        "{}",
        stderr(&output)
    );
    let (directory, report_path) = project("doctor_stale", Extras::default());
    fs::write(directory.join("app/Foo.php"), "<?php\n").unwrap();
    let output = doctor(&report_path, "text");
    assert_eq!(
        output.status.code(),
        Some(2),
        "a stale source is a hard error"
    );
}
