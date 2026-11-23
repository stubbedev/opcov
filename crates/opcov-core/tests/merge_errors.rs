#[cfg(test)]
mod refusals {
    use std::io;

    use opcov_core::report::{
        ConflictSite, HeaderField, InputId, MergeError, OutcomeField, Report, StreamMergeError,
        merge, merge_streams,
    };

    const BASE: &str = concat!(
        r#"{"schema":"opcov-report/1","#,
        r#""generator":{"opcov":"1.0.0","php":"8.5.12","zts":true,"vm":"HYBRID"},"#,
        r#""settings":{"collect_paths":["app"],"exclude_paths":[],"exclude_pattern":null},"#,
        r#""paths":{"mode":"relative","root":"/srv/app"},"#,
        r#""tests":["Tests\\FooTest::test_a"],"#,
        r#""files":{"app/Foo.php":{"#,
        r#""sha256":"e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855","#,
        r#""lines":{"12":[0]},"#,
        r#""functions":{"App\\Foo::bar":{"span":null,"outcomes":["#,
        r#"{"id":"o1","kind":"IfTrue","line":12,"span":null,"hits":[0]}],"#,
        r#""loops":[]}}}}}"#,
        "\n"
    );

    fn variant(from: &str, to: &str) -> String {
        assert!(BASE.contains(from), "the base report contains {from:?}");
        return BASE.replacen(from, to, 1);
    }

    fn report(document: &str) -> Report {
        return Report::read(document.as_bytes()).unwrap();
    }

    fn stream(documents: &[String]) -> Result<Vec<u8>, StreamMergeError> {
        let inputs: Vec<&[u8]> = documents.iter().map(String::as_bytes).collect();
        return merge_streams(&inputs, |input| return io::Result::Ok(*input), Vec::new());
    }

    #[test]
    fn differing_generator_versions_are_refused_naming_field_and_values() {
        let other = variant(r#""opcov":"1.0.0""#, r#""opcov":"1.1.0""#);
        let error = merge(&[report(BASE), report(&other)]).unwrap_err();
        let MergeError::Header(mismatch) = &error else {
            panic!("expected a header mismatch, got {error}");
        };
        assert_eq!(
            mismatch.field,
            HeaderField::GeneratorOpcov,
            "the field is named"
        );
        assert_eq!(mismatch.left, r#""1.0.0""#, "the first value is named");
        assert_eq!(mismatch.right, r#""1.1.0""#, "the second value is named");
        assert_eq!(
            mismatch.second,
            InputId::new(1),
            "the second input is named"
        );
        assert!(error.is_refusal(), "a mismatch is a refusal");
        let message = error.describe(&["a.json", "b.json"]).to_string();
        assert_eq!(
            message,
            r#"refusing to merge: generator.opcov differs: "1.0.0" in a.json but "1.1.0" in b.json"#,
            "the message names field, values and inputs"
        );
    }

    #[test]
    fn differing_settings_and_paths_are_refused() {
        let cases = [
            (
                variant(r#""exclude_pattern":null"#, r#""exclude_pattern":"~x~""#),
                HeaderField::SettingsExcludePattern,
            ),
            (
                variant(
                    r#""collect_paths":["app"]"#,
                    r#""collect_paths":["app","src"]"#,
                ),
                HeaderField::SettingsCollectPaths,
            ),
            (
                variant(r#""root":"/srv/app""#, r#""root":"/srv/other""#),
                HeaderField::Paths,
            ),
            (
                variant(r#""php":"8.5.12""#, r#""php":"8.4.1""#),
                HeaderField::GeneratorPhp,
            ),
        ];
        for (other, field) in cases {
            let error = merge(&[report(BASE), report(&other)]).unwrap_err();
            let MergeError::Header(mismatch) = &error else {
                panic!("expected a header mismatch on {field}, got {error}");
            };
            assert_eq!(mismatch.field, field, "the mismatching field is named");
        }
    }

    #[test]
    fn a_different_sha256_for_the_same_path_is_refused_naming_the_path() {
        let other = variant("e3b0c442", "00b0c442");
        let error = merge(&[report(BASE), report(&other)]).unwrap_err();
        let MergeError::Conflict(conflict) = &error else {
            panic!("expected a conflict, got {error}");
        };
        assert_eq!(conflict.path.as_str(), "app/Foo.php", "the path is named");
        assert_eq!(conflict.site, ConflictSite::Sha256, "the field is named");
        assert!(
            error.to_string().contains("app/Foo.php"),
            "{error} names the path"
        );
    }

    #[test]
    fn differing_outcome_models_are_refused() {
        let other = variant(r#""kind":"IfTrue""#, r#""kind":"IfFalse""#);
        let error = merge(&[report(BASE), report(&other)]).unwrap_err();
        let MergeError::Conflict(conflict) = &error else {
            panic!("expected a conflict, got {error}");
        };
        assert!(
            matches!(
                conflict.site,
                ConflictSite::Outcome {
                    field: OutcomeField::Kind,
                    ..
                }
            ),
            "the outcome kind is named, got {error}"
        );
    }

    #[test]
    fn no_inputs_is_an_error() {
        let error = merge(&[]).unwrap_err();
        assert_eq!(error, MergeError::NoInputs, "merging nothing is refused");
    }

    #[test]
    fn streaming_refuses_an_unknown_schema_version_naming_the_input() {
        let future = r#"{"schema":"opcov-report/2"}"#.to_owned();
        let error = stream(&[BASE.to_owned(), future]).unwrap_err();
        assert!(
            error.is_refusal(),
            "an unknown major version is a refusal: {error}"
        );
        let message = error.describe(&["a.json", "b.json"]).to_string();
        assert!(message.starts_with("b.json: "), "{message} names the input");
        assert!(
            message.contains("opcov-report/2"),
            "{message} names the version"
        );
    }

    #[test]
    fn streaming_reports_invalid_input_with_its_name() {
        let broken = variant("[0]}", "[1]}");
        let error = stream(&[BASE.to_owned(), broken]).unwrap_err();
        assert!(
            !error.is_refusal(),
            "an invalid input is not a refusal: {error}"
        );
        let message = error.describe(&["a.json", "b.json"]).to_string();
        assert!(message.starts_with("b.json: "), "{message} names the input");
        assert!(
            message.contains("references test 1"),
            "{message} names the problem"
        );
    }

    #[test]
    fn streaming_refuses_conflicts_like_the_in_memory_merge() {
        let other = variant("e3b0c442", "00b0c442");
        let error = stream(&[BASE.to_owned(), other]).unwrap_err();
        assert!(
            error.is_refusal(),
            "a sha256 conflict is a refusal: {error}"
        );
        let message = error.describe(&["a.json", "b.json"]).to_string();
        assert!(message.contains("app/Foo.php"), "{message} names the path");
        assert!(
            message.contains("a.json") && message.contains("b.json"),
            "{message} names both inputs"
        );
    }

    #[test]
    fn streaming_reports_an_input_that_cannot_be_opened() {
        let inputs = ["missing.json"];
        let error = merge_streams(
            &inputs,
            |_name| return Err::<&[u8], _>(io::Error::from(io::ErrorKind::NotFound)),
            Vec::new(),
        )
        .unwrap_err();
        let message = error.describe(&inputs).to_string();
        assert!(
            message.starts_with("missing.json: cannot open"),
            "{message} names the input"
        );
        assert!(!error.is_refusal(), "an unreadable input is not a refusal");
    }
}
