#[cfg(test)]
mod plan {
    use std::fs;
    use std::path::Path;

    use std::collections::BTreeSet;

    use opcov_core::cfg::{
        Analysis, BlockFlag, BlockFlags, BlockId, BlockInput, FunctionFlags, FunctionInput, Line,
        LoopInput, LoopShape, LoopTest, Op, Opcode, Origin, Plan,
    };
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

    fn block(start: bool, ops: &[Opcode], successors: &[u32]) -> BlockInput {
        let flags = BlockFlags::empty().with(BlockFlag::Reachable);
        return BlockInput {
            flags: if start {
                flags.with(BlockFlag::Start)
            } else {
                flags
            },
            ops: ops
                .iter()
                .map(|&opcode| {
                    return Op {
                        opcode,
                        line: Line::new(3).unwrap(),
                    };
                })
                .collect(),
            successors: successors.iter().copied().map(BlockId::new).collect(),
        };
    }

    #[test]
    fn planning_many_loops_in_one_function_stays_linear() {
        let count: u32 = 20_000;
        let mut blocks = Vec::new();
        let mut loops = Vec::new();
        for index in 0..count {
            let entry = index * 3;
            blocks.push(block(index == 0, &[Opcode::Jmp], &[entry + 2]));
            blocks.push(block(false, &[Opcode::PreDec], &[entry + 2]));
            blocks.push(block(
                false,
                &[Opcode::IsSmaller, Opcode::Jmpnz],
                &[entry + 1, entry + 3],
            ));
            loops.push(LoopInput {
                header: BlockId::new(entry + 2),
                body_blocks: BTreeSet::from([BlockId::new(entry + 1)]),
            });
        }
        blocks.push(block(false, &[Opcode::Return], &[]));
        let analysis = Analysis::try_from(FunctionInput {
            key: "f".parse().unwrap(),
            file: "/app/f.php".parse().unwrap(),
            line_start: Line::new(2).unwrap(),
            line_end: Line::new(9).unwrap(),
            flags: FunctionFlags::empty(),
            blocks,
            loops,
        })
        .unwrap();
        let plan = Plan::of(&analysis.function);
        assert_eq!(plan.loops().len(), 20_000, "every loop is planned");
        for (index, planned) in (0..).zip(plan.loops()) {
            let LoopTest::PreTest {
                test,
                back,
                exit,
                entries,
            } = &planned.test
            else {
                panic!("loop {index} is not a pre-test loop: {:?}", planned.test);
            };
            assert_eq!(test.get(), index * 3 + 2, "loop {index}: its test block");
            assert_eq!(
                entries,
                &[BlockId::new(index * 3)],
                "loop {index}: its entry jump"
            );
            assert_eq!(plan.outcome(*back).unwrap().kind, EdgeKind::LoopBack);
            assert_eq!(plan.outcome(*exit).unwrap().kind, EdgeKind::LoopExit);
        }
    }
}
