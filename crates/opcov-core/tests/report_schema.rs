#[cfg(test)]
#[path = "support/arbitrary_report.rs"]
mod arbitrary_report;

#[cfg(test)]
mod schema {
    use opcov_core::kind::EdgeKind;
    use opcov_core::report::{ReadError, Report, SchemaError, schema_id};
    use proptest::prelude::*;
    use serde_json::Value;

    use super::arbitrary_report::{UNIT_OUTCOME_KINDS, bytes, run, universe};

    const SCHEMA: &str = include_str!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../schema/opcov-report.v1.json"
    ));

    const EXAMPLE: &str = concat!(
        r#"{"schema":"opcov-report/1","#,
        r#""generator":{"opcov":"1.0.0","php":"8.5.12","zts":true,"vm":"HYBRID"},"#,
        r#""settings":{"collect_paths":["app"],"exclude_paths":[],"exclude_pattern":null,"strict":true},"#,
        r#""paths":{"mode":"relative","root":"/srv/app"},"#,
        r#""tests":["Tests\\Unit\\FooTest::test_bar","Tests\\Unit\\FooTest::test_baz"],"#,
        r#""files":{"app/Foo.php":{"#,
        r#""sha256":"e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855","#,
        r#""lines":{"12":[0,1],"14":[]},"#,
        r#""functions":{"App\\Foo::bar":{"span":[10,1,30,5],"outcomes":["#,
        r#"{"id":"o7","kind":"CoalesceFallback","line":14,"span":[14,20,14,31],"hits":[0]},"#,
        r#"{"id":"o8","kind":{"MatchArm":2},"line":15,"span":null,"hits":[]}],"#,
        r#""loops":[{"id":"l1","kind":"Foreach","line":18,"zero":[],"second":[1]}]}}}}}"#,
        "\n"
    );

    fn validator() -> jsonschema::Validator {
        let schema: Value = serde_json::from_str(SCHEMA).unwrap();
        jsonschema::draft202012::meta::validate(&schema).unwrap();
        return jsonschema::draft202012::new(&schema).unwrap();
    }

    fn schema_accepts(document: &str) -> bool {
        let instance: Value = serde_json::from_str(document).unwrap();
        return validator().is_valid(&instance);
    }

    fn mutate(from: &str, to: &str) -> String {
        assert!(EXAMPLE.contains(from), "the example contains {from:?}");
        return EXAMPLE.replacen(from, to, 1);
    }

    fn assert_parser_rejects(name: &str, document: &str) {
        if let Ok(report) = Report::read(document.as_bytes()) {
            panic!("the parser accepted {name}: {report:?}");
        }
    }

    const fn unit_position(kind: EdgeKind) -> Option<usize> {
        return match kind {
            EdgeKind::IfTrue => Some(0),
            EdgeKind::IfFalse => Some(1),
            EdgeKind::ShortCircuit => Some(2),
            EdgeKind::Evaluate => Some(3),
            EdgeKind::CoalesceValue => Some(4),
            EdgeKind::CoalesceFallback => Some(5),
            EdgeKind::TernaryValue => Some(6),
            EdgeKind::TernaryFallback => Some(7),
            EdgeKind::NullsafeNull => Some(8),
            EdgeKind::NullsafeValue => Some(9),
            EdgeKind::MatchNoArm => Some(10),
            EdgeKind::SwitchDefault => Some(11),
            EdgeKind::LoopEnter => Some(12),
            EdgeKind::LoopSkip => Some(13),
            EdgeKind::LoopBack => Some(14),
            EdgeKind::LoopExit => Some(15),
            EdgeKind::Catch => Some(16),
            EdgeKind::Finally => Some(17),
            EdgeKind::Engine => Some(18),
            EdgeKind::Jump => Some(19),
            EdgeKind::Flow | EdgeKind::MatchArm(_) | EdgeKind::SwitchCase(_) => None,
        };
    }

    #[test]
    fn every_unit_outcome_kind_is_generated_and_in_the_schema() {
        for (position, kind) in UNIT_OUTCOME_KINDS.iter().enumerate() {
            assert_eq!(
                unit_position(*kind),
                Some(position),
                "{kind:?} is listed once"
            );
            let name = serde_json::to_string(kind).unwrap();
            let document = mutate(r#""CoalesceFallback""#, &name);
            assert!(schema_accepts(&document), "the schema accepts {name}");
            Report::read(document.as_bytes()).unwrap();
        }
    }

    #[test]
    fn the_example_is_canonical_and_valid() {
        assert!(schema_accepts(EXAMPLE), "the schema accepts the example");
        let report = Report::read(EXAMPLE.as_bytes()).unwrap();
        assert_eq!(
            String::from_utf8(bytes(&report)).unwrap(),
            EXAMPLE,
            "round trip is byte-identical"
        );
        assert_eq!(report.header().tests.len(), 2, "both tests are read");
    }

    #[test]
    fn rejected_by_schema_and_parser() {
        let sha = "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855";
        let cases = [
            (
                "unknown member",
                mutate(r#""vm":"HYBRID""#, r#""vm":"HYBRID","jit":true"#),
            ),
            ("unknown vm", mutate(r#""vm":"HYBRID""#, r#""vm":"JIT""#)),
            ("uppercase sha256", mutate(sha, &sha.to_uppercase())),
            ("short sha256", mutate(sha, "e3b0")),
            ("padded outcome id", mutate(r#""id":"o7""#, r#""id":"o07""#)),
            (
                "loop id with outcome prefix",
                mutate(r#""id":"l1""#, r#""id":"o1""#),
            ),
            ("line key zero", mutate(r#""12":[0,1]"#, r#""0":[0,1]"#)),
            ("line key padded", mutate(r#""12":[0,1]"#, r#""012":[0,1]"#)),
            (
                "span with column zero",
                mutate("[14,20,14,31]", "[14,0,14,31]"),
            ),
            (
                "span with three numbers",
                mutate("[14,20,14,31]", "[14,20,14]"),
            ),
            (
                "flow is not an outcome",
                mutate(r#""CoalesceFallback""#, r#""Flow""#),
            ),
            (
                "parent segment in path",
                mutate(r#""app/Foo.php""#, r#""app/../Foo.php""#),
            ),
            (
                "empty segment in path",
                mutate(r#""app/Foo.php""#, r#""app//Foo.php""#),
            ),
            (
                "relative mode without root",
                mutate(r#","root":"/srv/app""#, ""),
            ),
            (
                "absolute mode with root",
                mutate(r#""mode":"relative""#, r#""mode":"absolute""#),
            ),
            ("relative root", mutate(r#""/srv/app""#, r#""srv/app""#)),
            ("duplicate test", mutate("test_baz", "test_bar")),
            ("duplicate hit", mutate("[0,1]", "[1,1]")),
            ("negative hit", mutate("[0,1]", "[-1,1]")),
            (
                "missing exclude_pattern",
                mutate(r#""exclude_pattern":null,"#, ""),
            ),
            ("missing span", mutate(r#""span":null,"#, "")),
            (
                "empty test name",
                mutate(r#""Tests\\Unit\\FooTest::test_bar""#, r#""""#),
            ),
            ("empty function key", mutate(r#""App\\Foo::bar""#, r#""""#)),
            (
                "schema version 2",
                mutate("opcov-report/1", "opcov-report/2"),
            ),
        ];
        for (name, document) in cases {
            assert!(!schema_accepts(&document), "the schema rejects: {name}");
            assert_parser_rejects(name, &document);
        }
    }

    #[test]
    fn rejected_by_parser_beyond_the_schema() {
        let cases = [
            ("test index out of range", mutate("[0,1]", "[0,2]")),
            ("hits not ascending", mutate("[0,1]", "[1,0]")),
            ("tests not ascending", mutate("test_baz", "test_aaa")),
            ("lines not ascending", mutate(r#""14":[]"#, r#""11":[]"#)),
            (
                "outcomes not ascending",
                mutate(r#""id":"o8""#, r#""id":"o6""#),
            ),
            (
                "line outside span",
                mutate(r#""line":14,"span""#, r#""line":40,"span""#),
            ),
            (
                "span ends before it starts",
                mutate("[14,20,14,31]", "[14,31,14,20]"),
            ),
            (
                "absolute path in relative mode",
                mutate(r#""app/Foo.php""#, r#""/app/Foo.php""#),
            ),
            (
                "malformed function key",
                mutate(r#""App\\Foo::bar""#, r#""App\\Foo::ba-r""#),
            ),
            (
                "closure key without lines",
                mutate(r#""App\\Foo::bar""#, r#""{closure}@/srv/app/Foo.php""#),
            ),
            (
                "members out of order",
                mutate(
                    r#"{"schema":"opcov-report/1","#,
                    r#"{"tests":[],"schema":"opcov-report/1","#,
                ),
            ),
            (
                "files not ascending",
                mutate(
                    r#""files":{"app/Foo.php":{"#,
                    &format!(
                        r#""files":{{"b.php":{{"sha256":"{sha}","lines":{{}},"functions":{{}}}},"app/Foo.php":{{"#,
                        sha = "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"
                    ),
                ),
            ),
        ];
        for (name, document) in cases {
            assert!(schema_accepts(&document), "only the parser sees: {name}");
            assert_parser_rejects(name, &document);
        }
        assert_parser_rejects("trailing data", &format!("{EXAMPLE}{{}}"));
    }

    #[test]
    fn functions_are_read_in_any_order_and_written_in_key_order() {
        let unordered = mutate(
            r#""functions":{"#,
            r#""functions":{"App\Foo::zed":{"span":null,"outcomes":[],"loops":[]},"#,
        );
        let report = Report::read(unordered.as_bytes()).unwrap();
        let written = String::from_utf8(bytes(&report)).unwrap();
        let bar = written.find("App\\Foo::bar").unwrap();
        let zed = written.find("App\\Foo::zed").unwrap();
        assert!(bar < zed, "functions are written in key order: {written}");
        assert_eq!(Report::read(written.as_bytes()).unwrap(), report, "the reordered report reads back");
    }

    #[test]
    fn unknown_major_versions_are_rejected_before_anything_else() {
        let future = r#"{"schema":"opcov-report/2","entirely":"different"}"#;
        let error = Report::read(future.as_bytes()).unwrap_err();
        let ReadError::Schema(SchemaError::Unsupported { found }) = &error else {
            panic!("expected an unsupported version, got {error}");
        };
        assert_eq!(found, "opcov-report/2", "the error names the version found");
        let message = error.to_string();
        assert!(
            message.contains("opcov-report/2"),
            "{message} names the version found"
        );
        assert!(
            message.contains(&schema_id()),
            "{message} names the version supported"
        );

        let garbage = r#"{"schema":"coverage/1"}"#;
        let error = Report::read(garbage.as_bytes()).unwrap_err();
        assert!(
            matches!(error, ReadError::Schema(SchemaError::Invalid { .. })),
            "an unknown family is invalid, got {error}"
        );
    }

    proptest! {
        #[test]
        fn generated_reports_are_valid_canonical_and_round_trip(universe in universe(), run in run()) {
            let report = universe.report(&run);
            let first = bytes(&report);
            prop_assert_eq!(&first, &bytes(&report));
            let text = String::from_utf8(first.clone()).unwrap();
            prop_assert!(schema_accepts(&text), "schema rejects {}", text);
            let read = Report::read(first.as_slice()).unwrap();
            prop_assert_eq!(&read, &report);
            prop_assert_eq!(bytes(&read), first);
        }
    }
}
