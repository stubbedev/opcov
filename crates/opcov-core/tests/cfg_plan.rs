#[cfg(test)]
mod plan {
    use std::fs;
    use std::path::Path;

    use opcov_core::cfg::{Analysis, LoopShape, LoopTest, Origin, Plan};
    use opcov_core::kind::EdgeKind;

    fn plan(name: &str) -> Plan {
        let path = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests/golden/cfg")
            .join(format!("{name}.json"));
        let text = fs::read_to_string(&path).unwrap();
        let golden: serde_json::Value = serde_json::from_str(&text).unwrap();
        let analysis: Analysis = serde_json::from_value(golden["expected"].clone()).unwrap();
        return Plan::of(&analysis.function);
    }

    #[test]
    fn outcome_ids_follow_site_then_successor_order() {
        for name in [
            "and_or",
            "if_elseif_else",
            "try_catch_finally",
            "match_jumptable",
        ] {
            let plan = plan(name);
            let order: Vec<_> = plan
                .outcomes()
                .iter()
                .map(|outcome| return outcome.site)
                .collect();
            let mut sorted = order.clone();
            sorted.sort_unstable();
            assert_eq!(order, sorted, "{name}: sites ascend with ids");
            for (number, outcome) in (0..).zip(plan.outcomes()) {
                assert_eq!(outcome.id.get(), number, "{name}: ids are dense");
            }
        }
    }

    #[test]
    fn a_finally_block_has_an_exception_entry_outcome() {
        let plan = plan("try_catch_finally");
        let entries: Vec<_> = plan
            .outcomes()
            .iter()
            .filter(|outcome| return outcome.kind == EdgeKind::FinallyException)
            .collect();
        assert_eq!(entries.len(), 1, "one finally block, one exception entry");
        let entry = entries[0];
        assert!(
            matches!(entry.origin, Origin::FinallyEntry { .. }),
            "the entry is not an edge"
        );
        assert!(
            plan.outcomes()
                .iter()
                .any(|outcome| return outcome.kind == EdgeKind::Finally),
            "the normal entry stays an edge outcome"
        );
    }

    #[test]
    fn loops_are_planned_with_their_tests() {
        let shapes = [
            ("foreach", LoopShape::Foreach),
            ("for", LoopShape::PreTest),
            ("while", LoopShape::PreTest),
            ("do_while", LoopShape::PostTest),
        ];
        for (name, shape) in shapes {
            let plan = plan(name);
            assert_eq!(plan.loops().len(), 1, "{name}: one loop");
            let planned = &plan.loops()[0];
            assert_eq!(planned.id.get(), 0, "{name}: first loop id");
            assert_eq!(planned.test.shape(), shape, "{name}: shape");
            let outcome_kind = |id| return plan.outcome(id).unwrap().kind;
            match &planned.test {
                LoopTest::Foreach { skip, back, exit } => {
                    assert_eq!(outcome_kind(*skip), EdgeKind::LoopSkip, "{name}");
                    assert_eq!(outcome_kind(*back), EdgeKind::LoopBack, "{name}");
                    assert_eq!(outcome_kind(*exit), EdgeKind::LoopExit, "{name}");
                }
                LoopTest::PreTest {
                    back,
                    exit,
                    entries,
                    ..
                } => {
                    assert_eq!(outcome_kind(*back), EdgeKind::LoopBack, "{name}");
                    assert_eq!(outcome_kind(*exit), EdgeKind::LoopExit, "{name}");
                    assert_eq!(entries.len(), 1, "{name}: one entry jump");
                }
                LoopTest::PostTest { back, .. } => {
                    assert_eq!(outcome_kind(*back), EdgeKind::LoopBack, "{name}");
                }
            }
        }
    }
}
