#[cfg(test)]
mod syntax {
    use std::fs;
    use std::path::{Path, PathBuf};

    use opcov_core::cfg::{Analysis, BlockId, FunctionKey, Line};
    use opcov_core::kind::{EdgeKind, LoopKind};
    use opcov_syntax::{
        BranchKind, ConstructKind, FunctionSource, Location, LookupError, OutcomeRef, SourceFile,
        SyntaxErrorKind, Unmatched, outcome_refs,
    };

    const FIXTURES: [&str; 5] = ["branches", "arms", "loops", "functions", "columns"];

    fn fixture_dir() -> PathBuf {
        return Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures");
    }

    fn parse(name: &str) -> SourceFile {
        let path = fixture_dir().join(format!("{name}.php"));
        let source = fs::read(&path).unwrap();
        return SourceFile::parse(format!("{name}.php").parse().unwrap(), &source).unwrap();
    }

    fn function<'a>(file: &'a SourceFile, key: &str) -> &'a FunctionSource {
        return file.function(&key.parse().unwrap()).unwrap();
    }

    fn line(number: u32) -> Line {
        return Line::new(number).unwrap();
    }

    fn outcome(source: u32, kind: EdgeKind, at: u32) -> OutcomeRef {
        return OutcomeRef {
            source: BlockId::new(source),
            kind,
            line: line(at),
        };
    }

    fn locate(
        function: &FunctionSource,
        outcomes: &[OutcomeRef],
    ) -> Vec<Result<Location, Unmatched>> {
        return function
            .match_outcomes(outcomes)
            .outcomes()
            .iter()
            .map(|matched| return matched.location)
            .collect();
    }

    fn located(function: &FunctionSource, outcome: OutcomeRef) -> Location {
        let mut found = locate(function, &[outcome]);
        return found.pop().unwrap().unwrap();
    }

    fn spans(location: Location) -> (String, String) {
        return (location.span.to_string(), location.outcome_span.to_string());
    }

    #[test]
    fn fixtures_produce_the_expected_functions_and_constructs() {
        for name in FIXTURES {
            let file = parse(name);
            let actual = serde_json::to_value(file.functions()).unwrap();
            let expected_path = fixture_dir().join(format!("{name}.json"));
            let expected: serde_json::Value =
                serde_json::from_str(&fs::read_to_string(&expected_path).unwrap()).unwrap();
            assert_eq!(
                actual,
                expected,
                "{name}: actual\n{}",
                serde_json::to_string_pretty(&actual).unwrap()
            );
        }
    }

    #[test]
    fn several_outcomes_on_one_line_match_in_compilation_order() {
        let file = parse("branches");
        let branches = function(&file, "branches");
        let coalesce = locate(
            branches,
            &[
                outcome(1, EdgeKind::CoalesceFallback, 16),
                outcome(2, EdgeKind::CoalesceValue, 16),
            ],
        );
        let outer = coalesce[0].unwrap();
        assert_eq!(
            outer.construct,
            ConstructKind::Branch(BranchKind::Coalesce),
            "outer kind"
        );
        assert_eq!(
            spans(outer).0,
            "16:10-16:24",
            "the first COALESCE is the outer `??`"
        );
        assert_eq!(
            spans(coalesce[1].unwrap()).0,
            "16:16-16:24",
            "the second COALESCE is the inner `??`"
        );
        let nullsafe = locate(
            branches,
            &[
                outcome(5, EdgeKind::NullsafeNull, 18),
                outcome(6, EdgeKind::NullsafeNull, 18),
            ],
        );
        assert_eq!(
            nullsafe[0].unwrap().construct,
            ConstructKind::Branch(BranchKind::NullsafeProperty),
            "the object's `?->` is compiled first"
        );
        assert_eq!(
            nullsafe[1].unwrap().construct,
            ConstructKind::Branch(BranchKind::NullsafeMethodCall),
            "the call's `?->` is compiled second"
        );
    }

    #[test]
    fn short_circuits_and_conditions_on_one_line_are_told_apart() {
        let file = parse("branches");
        let branches = function(&file, "branches");
        let found = locate(
            branches,
            &[
                outcome(3, EdgeKind::ShortCircuit, 6),
                outcome(4, EdgeKind::IfTrue, 6),
                outcome(7, EdgeKind::Evaluate, 19),
                outcome(8, EdgeKind::Evaluate, 19),
            ],
        );
        let kinds: Vec<ConstructKind> = found
            .iter()
            .map(|result| return result.unwrap().construct)
            .collect();
        assert_eq!(
            kinds,
            vec![
                ConstructKind::Branch(BranchKind::And),
                ConstructKind::Branch(BranchKind::ElseIf),
                ConstructKind::Branch(BranchKind::LogicalAnd),
                ConstructKind::Branch(BranchKind::LogicalOr),
            ],
            "`and` inside the parentheses is compiled before `or`"
        );
    }

    #[test]
    fn nested_multi_line_ternaries_match_by_condition_line() {
        let file = parse("branches");
        let branches = function(&file, "branches");
        let outer = located(branches, outcome(9, EdgeKind::IfTrue, 20));
        let inner = located(branches, outcome(10, EdgeKind::IfFalse, 21));
        assert_eq!(
            spans(outer).0,
            "20:10-22:12",
            "outer ternary spans three lines"
        );
        assert_eq!(spans(inner).0, "21:12-21:22", "inner ternary");
    }

    #[test]
    fn unmatched_outcomes_carry_a_reason() {
        let file = parse("branches");
        let branches = function(&file, "branches");
        let found = locate(
            branches,
            &[
                outcome(1, EdgeKind::CoalesceValue, 16),
                outcome(2, EdgeKind::CoalesceValue, 16),
                outcome(3, EdgeKind::CoalesceValue, 16),
                outcome(4, EdgeKind::Engine, 16),
                outcome(5, EdgeKind::CoalesceValue, 4),
            ],
        );
        assert_eq!(
            found,
            vec![
                Err(Unmatched::Ambiguous {
                    sources: 3,
                    candidates: 2
                }),
                Err(Unmatched::Ambiguous {
                    sources: 3,
                    candidates: 2
                }),
                Err(Unmatched::Ambiguous {
                    sources: 3,
                    candidates: 2
                }),
                Err(Unmatched::NotFromSource),
                Err(Unmatched::NoConstruct),
            ],
            "ambiguity is reported, never guessed"
        );
        let matching = branches.match_outcomes(&[outcome(5, EdgeKind::CoalesceValue, 4)]);
        assert_eq!(
            matching.unmatched().count(),
            1,
            "the unmatched list names the outcome"
        );
    }

    #[test]
    fn match_arms_resolve_to_the_arm_in_source_position() {
        let file = parse("arms");
        let arms = function(&file, "arms");
        let default = located(arms, outcome(0, EdgeKind::MatchArm(1), 4));
        assert_eq!(
            spans(default).1,
            "6:9-6:27",
            "arm 1 is the default arm in the middle"
        );
        let found = locate(
            arms,
            &[
                outcome(0, EdgeKind::MatchArm(5), 4),
                outcome(0, EdgeKind::MatchNoArm, 4),
                outcome(1, EdgeKind::MatchNoArm, 10),
            ],
        );
        assert_eq!(
            found[0],
            Err(Unmatched::ArmOutOfRange { arm: 5, arms: 3 }),
            "an arm past the end is reported"
        );
        assert_eq!(
            found[1],
            Err(Unmatched::KindMismatch {
                construct: ConstructKind::Match
            }),
            "a match with a default arm cannot miss"
        );
        assert_eq!(spans(found[2].unwrap()).1, "9:10-12:6", "no arm matched");
    }

    #[test]
    fn switch_cases_resolve_through_shared_bodies_and_engine_quirks() {
        let file = parse("arms");
        let arms = function(&file, "arms");
        let shared = located(arms, outcome(2, EdgeKind::SwitchCase(0), 15));
        assert_eq!(spans(shared).1, "14:9-17:19", "cases 1 and 2 share a body");
        let fallthrough = located(arms, outcome(3, EdgeKind::SwitchCase(1), 19));
        assert_eq!(
            spans(fallthrough).1,
            "18:9-20:22",
            "default falls into case 3"
        );
        let end = located(arms, outcome(4, EdgeKind::SwitchCase(2), 21));
        assert_eq!(
            spans(end).1,
            "21:9-21:16",
            "the empty last case jumps to the end"
        );
        let default = located(arms, outcome(5, EdgeKind::SwitchDefault, 21));
        assert_eq!(spans(default).1, "18:9-18:17", "the default clause");
        let true_case = located(arms, outcome(6, EdgeKind::MatchArm(0), 24));
        assert_eq!(
            spans(true_case).1,
            "24:9-26:19",
            "switch (true) classified as a match chain"
        );
        let true_end = located(arms, outcome(7, EdgeKind::MatchArm(1), 24));
        assert_eq!(
            spans(true_end).1,
            "23:5-27:6",
            "no case of switch (true) matched"
        );
        let false_cases = locate(
            arms,
            &[
                outcome(8, EdgeKind::IfFalse, 29),
                outcome(9, EdgeKind::IfFalse, 32),
            ],
        );
        assert_eq!(
            false_cases
                .iter()
                .map(|result| return spans(result.unwrap()).1)
                .collect::<Vec<_>>(),
            vec!["29:9-31:19".to_owned(), "32:9-33:22".to_owned()],
            "switch (false) compiles like if/else and maps to its cases"
        );
        let wrong = locate(arms, &[outcome(10, EdgeKind::MatchArm(0), 14)]);
        assert_eq!(
            wrong[0],
            Err(Unmatched::KindMismatch {
                construct: ConstructKind::Switch
            }),
            "only switch (true) becomes a match chain"
        );
    }

    #[test]
    fn loops_catches_and_finally_paths_match() {
        let file = parse("loops");
        let loops = function(&file, "loops");
        let found = locate(
            loops,
            &[
                outcome(1, EdgeKind::LoopBack, 4),
                outcome(2, EdgeKind::LoopExit, 8),
                outcome(3, EdgeKind::LoopBack, 10),
                outcome(4, EdgeKind::LoopBack, 15),
                outcome(5, EdgeKind::LoopEnter, 16),
                outcome(6, EdgeKind::LoopBack, 16),
                outcome(7, EdgeKind::Finally, 20),
                outcome(8, EdgeKind::Catch, 21),
                outcome(9, EdgeKind::Catch, 21),
                outcome(10, EdgeKind::Catch, 23),
                outcome(11, EdgeKind::Finally, 25),
            ],
        );
        let kinds: Vec<ConstructKind> = found
            .iter()
            .map(|result| return result.unwrap().construct)
            .collect();
        assert_eq!(
            kinds,
            vec![
                ConstructKind::Loop(LoopKind::For),
                ConstructKind::Loop(LoopKind::For),
                ConstructKind::Loop(LoopKind::While),
                ConstructKind::Loop(LoopKind::DoWhile),
                ConstructKind::Loop(LoopKind::Foreach),
                ConstructKind::Loop(LoopKind::Foreach),
                ConstructKind::Finally,
                ConstructKind::Catch,
                ConstructKind::Catch,
                ConstructKind::Catch,
                ConstructKind::Finally,
            ],
            "every loop shape, catch type and finally path"
        );
        let types: Vec<String> = found[7..10]
            .iter()
            .map(|result| return spans(result.unwrap()).1)
            .collect();
        assert_eq!(
            types,
            vec!["21:14-21:29", "21:32-21:49", "23:14-23:24"],
            "each CATCH maps to its caught type"
        );
    }

    #[test]
    fn functions_are_keyed_like_the_engine() {
        let file = parse("functions");
        for (key, line) in [
            ("{closure}@functions.php:24-24", "24:29-24:46"),
            ("{closure}@functions.php:24-24#1", "24:69-24:86"),
            ("{closure}@functions.php:25-25", "25:19-25:49"),
            ("{closure}@functions.php:25-25#1", "25:30-25:49"),
            ("{closure}@functions.php:26-28", "26:20-28:10"),
            ("class@anonymous@functions.php:63-63::run", "63:22-63:62"),
            ("class@anonymous@functions.php:63-63#1::run", "63:88-63:123"),
        ] {
            assert_eq!(
                function(&file, key).span().to_string(),
                line,
                "{key} is the one at {line}: arrow functions and closures share one key kind, \
                 ranked by source position among the same line span"
            );
        }
        let arrow: FunctionKey = "{closure}@functions.php:24-24#2".parse().unwrap();
        assert_eq!(
            file.function(&arrow),
            Err(LookupError::NotFound { key: arrow.clone() }),
            "only two closures share line 24"
        );
        let missing: FunctionKey = "App\\Models\\User::missing".parse().unwrap();
        assert_eq!(
            file.function(&missing),
            Err(LookupError::NotFound {
                key: missing.clone()
            }),
            "unknown functions are reported"
        );
        let inner = function(&file, "{closure}@functions.php:25-25#1");
        let coalesce = located(inner, outcome(0, EdgeKind::CoalesceFallback, 25));
        assert_eq!(
            spans(coalesce).0,
            "25:41-25:49",
            "the nested arrow function owns the `??`"
        );
        let color = function(&file, "App\\Models\\Suit::color");
        assert_eq!(
            color.span().start().line().get(),
            52,
            "enum methods are keyed under the enum"
        );
    }

    #[test]
    fn same_span_declarations_are_keyed_like_the_engine_ranks_them() {
        let path = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../tests/phpt/keys/fixtures/ordinals.php");
        let source = fs::read(&path).unwrap();
        let file = SourceFile::parse("ordinals.php".parse().unwrap(), &source).unwrap();
        let mut keys: Vec<FunctionKey> = file
            .functions()
            .iter()
            .map(|function| return function.key().clone())
            .collect();
        keys.sort();
        let keys: Vec<String> = keys.iter().map(ToString::to_string).collect();
        assert_eq!(
            keys,
            [
                "{main}@ordinals.php",
                "holder",
                "class@anonymous@ordinals.php:6-6::run",
                "class@anonymous@ordinals.php:6-6#1::run",
                "class@anonymous@ordinals.php:7-7::make",
                "class@anonymous@ordinals.php:7-7#1::run",
                "{closure}@ordinals.php:2-2",
                "{closure}@ordinals.php:2-2#1",
                "{closure}@ordinals.php:3-3",
                "{closure}@ordinals.php:3-3#1",
                "{closure}@ordinals.php:4-4",
                "{closure}@ordinals.php:4-4#1",
                "{closure}@ordinals.php:5-5",
                "{closure}@ordinals.php:5-5#1",
                "{closure}@ordinals.php:8-8",
                "{closure}@ordinals.php:8-8#1",
            ],
            "the keys tests/phpt/keys/ordinals.phpt expects from the engine"
        );
        assert_eq!(
            function(&file, "{closure}@ordinals.php:4-4")
                .span()
                .to_string(),
            "4:11-4:30",
            "the outer arrow function ranks first"
        );
    }

    #[test]
    fn multibyte_and_heredoc_text_keeps_byte_columns() {
        let file = parse("columns");
        let columns = function(&file, "columns");
        let found = locate(
            columns,
            &[
                outcome(0, EdgeKind::CoalesceValue, 6),
                outcome(1, EdgeKind::CoalesceValue, 10),
                outcome(2, EdgeKind::CoalesceValue, 10),
                outcome(3, EdgeKind::CoalesceValue, 11),
            ],
        );
        let located: Vec<String> = found
            .iter()
            .map(|result| return spans(result.unwrap()).0)
            .collect();
        assert_eq!(
            located,
            vec!["6:15-6:26", "10:21-10:31", "10:36-10:45", "11:12-11:37"],
            "columns count bytes after heredocs, multibyte strings and the pipe operator"
        );
    }

    #[test]
    fn parse_failures_name_the_file_and_position() {
        let error =
            SourceFile::parse("broken.php".parse().unwrap(), b"<?php\nif ($a {\n").unwrap_err();
        assert_eq!(
            error.path().as_str(),
            "broken.php",
            "the error names the file"
        );
        let SyntaxErrorKind::Parse { position, .. } = error.kind() else {
            panic!("expected a parse error, got {error}");
        };
        assert_eq!(position.line().get(), 2, "the error names the line");
    }

    #[test]
    fn unknown_property_hooks_are_rejected() {
        let source = b"<?php\nclass A {\n    public int $x {\n        reset => 1;\n    }\n}\n";
        let result = SourceFile::parse("hook.php".parse().unwrap(), source);
        if let Err(error) = result {
            assert!(
                matches!(
                    error.kind(),
                    SyntaxErrorKind::UnknownPropertyHook { .. } | SyntaxErrorKind::Parse { .. }
                ),
                "unexpected error {error}"
            );
            return;
        }
        panic!("a hook named `reset` must not be keyed");
    }

    fn golden_paths() -> Vec<PathBuf> {
        let directory =
            Path::new(env!("CARGO_MANIFEST_DIR")).join("../opcov-core/tests/golden/cfg");
        let mut paths: Vec<PathBuf> = fs::read_dir(&directory)
            .unwrap()
            .map(|entry| return entry.unwrap().path())
            .filter(|path| {
                return path
                    .extension()
                    .is_some_and(|extension| return extension == "json");
            })
            .collect();
        paths.sort();
        return paths;
    }

    const fn compatible(edge: EdgeKind, construct: ConstructKind) -> bool {
        return match edge {
            EdgeKind::IfTrue | EdgeKind::IfFalse => matches!(
                construct,
                ConstructKind::Branch(BranchKind::If | BranchKind::ElseIf | BranchKind::Ternary)
                    | ConstructKind::Switch
            ),
            EdgeKind::ShortCircuit | EdgeKind::Evaluate => matches!(
                construct,
                ConstructKind::Branch(
                    BranchKind::And
                        | BranchKind::Or
                        | BranchKind::LogicalAnd
                        | BranchKind::LogicalOr
                )
            ),
            EdgeKind::CoalesceValue | EdgeKind::CoalesceFallback => matches!(
                construct,
                ConstructKind::Branch(BranchKind::Coalesce | BranchKind::CoalesceAssign)
            ),
            EdgeKind::TernaryValue | EdgeKind::TernaryFallback => {
                matches!(construct, ConstructKind::Branch(BranchKind::ShortTernary))
            }
            EdgeKind::NullsafeNull | EdgeKind::NullsafeValue => matches!(
                construct,
                ConstructKind::Branch(
                    BranchKind::NullsafeProperty | BranchKind::NullsafeMethodCall
                )
            ),
            EdgeKind::MatchArm(_) | EdgeKind::MatchNoArm => {
                matches!(construct, ConstructKind::Match | ConstructKind::Switch)
            }
            EdgeKind::SwitchCase(_) | EdgeKind::SwitchDefault => {
                matches!(construct, ConstructKind::Switch)
            }
            EdgeKind::LoopEnter | EdgeKind::LoopSkip | EdgeKind::LoopBack | EdgeKind::LoopExit => {
                matches!(construct, ConstructKind::Loop(_))
            }
            EdgeKind::Catch => matches!(construct, ConstructKind::Catch),
            EdgeKind::Finally => matches!(construct, ConstructKind::Finally),
            EdgeKind::Flow | EdgeKind::Engine | EdgeKind::Jump => false,
        };
    }

    fn expected_loops(name: &str) -> Vec<LoopKind> {
        return match name {
            "do_while" => vec![LoopKind::DoWhile],
            "for" => vec![LoopKind::For],
            "foreach" | "foreach_by_ref" => vec![LoopKind::Foreach],
            "nested_loops_break" => vec![LoopKind::Foreach, LoopKind::While],
            "while" => vec![LoopKind::While],
            _ => Vec::new(),
        };
    }

    #[test]
    fn every_outcome_of_the_cfg_goldens_carries_a_construct_and_span() {
        let paths = golden_paths();
        assert!(!paths.is_empty(), "the CFG goldens are present");
        for path in paths {
            let golden: serde_json::Value =
                serde_json::from_str(&fs::read_to_string(&path).unwrap()).unwrap();
            let name = path.file_stem().unwrap().to_str().unwrap().to_owned();
            let source = golden["source"].as_str().unwrap();
            let file_path = golden["input"]["file"].as_str().unwrap();
            let analysis: Analysis = serde_json::from_value(golden["expected"].clone()).unwrap();
            let file = SourceFile::parse(file_path.parse().unwrap(), source.as_bytes()).unwrap();
            let cfg = &analysis.function;
            let function = file.function(cfg.key()).unwrap();
            let outcomes = outcome_refs(cfg);
            let matching = function.match_outcomes(&outcomes);
            for matched in matching.outcomes() {
                let location = matched.location.unwrap_or_else(|reason| {
                    panic!("{name}: {:?} unmatched: {reason}", matched.outcome)
                });
                assert!(
                    compatible(matched.outcome.kind, location.construct),
                    "{name}: {:?} matched a {:?}",
                    matched.outcome,
                    location.construct
                );
                assert!(
                    location.span.contains_line(matched.outcome.line),
                    "{name}: {:?} lies outside {}",
                    matched.outcome,
                    location.span
                );
            }
            let loops: Vec<LoopKind> = cfg
                .loops()
                .iter()
                .map(|cfg_loop| return matching.loop_kind(cfg, cfg_loop).unwrap())
                .collect();
            assert_eq!(loops, expected_loops(&name), "{name}: loop kinds");
        }
    }
}
