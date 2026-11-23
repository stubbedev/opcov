#[cfg(test)]
mod model {
    use std::collections::BTreeSet;

    use opcov_core::cfg::{
        Analysis, BlockFlag, BlockFlags, BlockId, BlockInput, CfgError, Diagnostic, Function,
        FunctionFlag, FunctionFlags, FunctionInput, Line, LoopInput, LoopShape, Op, OpIndex,
        Opcode, Successor, opcode_edge_kind,
    };
    use opcov_core::kind::EdgeKind;

    const ZEND_BB_START: u32 = 1;
    const ZEND_BB_TARGET: u32 = 1 << 2;
    const ZEND_BB_CATCH: u32 = 1 << 6;
    const ZEND_BB_FINALLY: u32 = 1 << 7;
    const ZEND_BB_FINALLY_END: u32 = 1 << 8;
    const ZEND_BB_REACHABLE: u32 = 1 << 31;
    const ZEND_ACC_STATIC: u32 = 1 << 4;
    const ZEND_ACC_ABSTRACT: u32 = 1 << 6;
    const ZEND_ACC_RETURN_REFERENCE: u32 = 1 << 12;
    const ZEND_ACC_CLOSURE: u32 = 1 << 22;
    const ZEND_ACC_GENERATOR: u32 = 1 << 24;

    fn line(number: u32) -> Line {
        return Line::new(number).expect("line numbers start at 1");
    }

    fn reachable(ops: &[Opcode], successors: &[u32]) -> BlockInput {
        return BlockInput {
            flags: BlockFlags::empty().with(BlockFlag::Reachable),
            ops: ops
                .iter()
                .map(|&opcode| {
                    return Op {
                        opcode,
                        line: line(3),
                    };
                })
                .collect(),
            successors: successors.iter().copied().map(BlockId::new).collect(),
        };
    }

    fn entry(ops: &[Opcode], successors: &[u32]) -> BlockInput {
        let mut block = reachable(ops, successors);
        block.flags = block.flags.with(BlockFlag::Start);
        return block;
    }

    fn function(blocks: Vec<BlockInput>, loops: Vec<LoopInput>) -> FunctionInput {
        return FunctionInput {
            key: "App\\f".parse().expect("valid key"),
            file: "/app/f.php".parse().expect("valid path"),
            line_start: line(2),
            line_end: line(9),
            flags: FunctionFlags::empty(),
            blocks,
            loops,
        };
    }

    fn analyze(blocks: Vec<BlockInput>, loops: Vec<LoopInput>) -> Analysis {
        return Analysis::try_from(function(blocks, loops)).expect("valid snapshot");
    }

    fn rejection(blocks: Vec<BlockInput>, loops: Vec<LoopInput>) -> CfgError {
        return Analysis::try_from(function(blocks, loops))
            .expect_err("invalid snapshot")
            .error();
    }

    fn edges(analysis: &Analysis) -> Vec<(u32, u32, EdgeKind)> {
        return analysis
            .function
            .edges()
            .iter()
            .map(|edge| return (edge.from.get(), edge.to.get(), edge.kind))
            .collect();
    }

    fn body(blocks: &[u32]) -> BTreeSet<BlockId> {
        return blocks.iter().copied().map(BlockId::new).collect();
    }

    #[test]
    fn opcode_successors_map_to_outcome_kinds() {
        let cases = [
            (Opcode::Jmpz, Successor::Target, Some(EdgeKind::IfFalse)),
            (Opcode::Jmpz, Successor::Fallthrough, Some(EdgeKind::IfTrue)),
            (Opcode::Jmpnz, Successor::Target, Some(EdgeKind::IfTrue)),
            (
                Opcode::Jmpnz,
                Successor::Fallthrough,
                Some(EdgeKind::IfFalse),
            ),
            (
                Opcode::JmpzEx,
                Successor::Target,
                Some(EdgeKind::ShortCircuit),
            ),
            (
                Opcode::JmpnzEx,
                Successor::Fallthrough,
                Some(EdgeKind::Evaluate),
            ),
            (
                Opcode::Coalesce,
                Successor::Target,
                Some(EdgeKind::CoalesceValue),
            ),
            (
                Opcode::Coalesce,
                Successor::Fallthrough,
                Some(EdgeKind::CoalesceFallback),
            ),
            (
                Opcode::JmpSet,
                Successor::Target,
                Some(EdgeKind::TernaryValue),
            ),
            (
                Opcode::JmpSet,
                Successor::Fallthrough,
                Some(EdgeKind::TernaryFallback),
            ),
            (
                Opcode::JmpNull,
                Successor::Target,
                Some(EdgeKind::NullsafeNull),
            ),
            (
                Opcode::JmpNull,
                Successor::Fallthrough,
                Some(EdgeKind::NullsafeValue),
            ),
            (
                Opcode::FeResetRw,
                Successor::Target,
                Some(EdgeKind::LoopSkip),
            ),
            (
                Opcode::FeResetR,
                Successor::Fallthrough,
                Some(EdgeKind::LoopEnter),
            ),
            (
                Opcode::FeFetchR,
                Successor::Target,
                Some(EdgeKind::LoopExit),
            ),
            (
                Opcode::FeFetchRw,
                Successor::Fallthrough,
                Some(EdgeKind::LoopBack),
            ),
            (
                Opcode::Match,
                Successor::Arm(3),
                Some(EdgeKind::MatchArm(3)),
            ),
            (
                Opcode::Match,
                Successor::Default,
                Some(EdgeKind::MatchNoArm),
            ),
            (Opcode::Match, Successor::Fallthrough, None),
            (
                Opcode::SwitchString,
                Successor::Arm(1),
                Some(EdgeKind::SwitchCase(1)),
            ),
            (
                Opcode::SwitchLong,
                Successor::Default,
                Some(EdgeKind::SwitchDefault),
            ),
            (
                Opcode::SwitchLong,
                Successor::Fallthrough,
                Some(EdgeKind::Engine),
            ),
            (Opcode::Catch, Successor::Fallthrough, Some(EdgeKind::Catch)),
            (Opcode::Catch, Successor::Target, Some(EdgeKind::Flow)),
            (Opcode::FastCall, Successor::Target, Some(EdgeKind::Finally)),
            (
                Opcode::FastCall,
                Successor::Fallthrough,
                Some(EdgeKind::Flow),
            ),
            (
                Opcode::JmpFrameless,
                Successor::Target,
                Some(EdgeKind::Engine),
            ),
            (
                Opcode::JmpFrameless,
                Successor::Fallthrough,
                Some(EdgeKind::Engine),
            ),
            (
                Opcode::AssertCheck,
                Successor::Target,
                Some(EdgeKind::Engine),
            ),
            (
                Opcode::BindInitStaticOrJmp,
                Successor::Fallthrough,
                Some(EdgeKind::Engine),
            ),
            (Opcode::Jmp, Successor::Target, Some(EdgeKind::Flow)),
            (Opcode::Jmp, Successor::Fallthrough, None),
            (Opcode::Echo, Successor::Fallthrough, Some(EdgeKind::Flow)),
            (Opcode::Echo, Successor::Target, None),
            (Opcode::Return, Successor::Fallthrough, None),
            (Opcode::Jmpz, Successor::Arm(0), None),
            (
                Opcode::Unknown(250),
                Successor::Fallthrough,
                Some(EdgeKind::Flow),
            ),
            (Opcode::Unknown(250), Successor::Target, None),
        ];
        for (opcode, successor, kind) in cases {
            assert_eq!(
                opcode_edge_kind(opcode, successor),
                kind,
                "{opcode:?} via {successor:?}"
            );
        }
    }

    #[test]
    fn blocks_tile_the_opcodes_and_take_their_line_range() {
        let mut first = entry(&[Opcode::Recv, Opcode::Jmpz], &[2, 1]);
        first.ops[1].line = line(5);
        let mut second = reachable(&[Opcode::Echo], &[2]);
        second.ops[0].line = line(4);
        let analysis = analyze(
            vec![
                first,
                second,
                reachable(&[Opcode::Echo, Opcode::Return], &[]),
            ],
            vec![],
        );
        let ranges: Vec<(u32, u32, u32, u32)> = analysis
            .function
            .blocks()
            .iter()
            .map(|block| {
                return (
                    block.op_start().get(),
                    block.op_end().get(),
                    block.line_start().get(),
                    block.line_end().get(),
                );
            })
            .collect();
        assert_eq!(
            ranges,
            [(0, 1, 3, 5), (2, 2, 4, 4), (3, 4, 3, 3)],
            "op and line ranges"
        );
        assert_eq!(
            edges(&analysis),
            [
                (0, 1, EdgeKind::IfTrue),
                (0, 2, EdgeKind::IfFalse),
                (1, 2, EdgeKind::Flow)
            ],
            "if without else"
        );
    }

    #[test]
    fn identical_targets_keep_both_outcomes() {
        let analysis = analyze(
            vec![
                entry(&[Opcode::Jmpz], &[1, 1]),
                reachable(&[Opcode::Return], &[]),
            ],
            vec![],
        );
        assert_eq!(
            edges(&analysis),
            [(0, 1, EdgeKind::IfTrue), (0, 1, EdgeKind::IfFalse)],
            "an empty if body still has two outcomes"
        );
    }

    #[test]
    fn switch_jumptable_ranks_cases_and_marks_the_type_fallback_as_engine() {
        let analysis = analyze(
            vec![
                entry(&[Opcode::SwitchLong], &[2, 3, 2, 4, 1]),
                reachable(&[Opcode::Jmp], &[4]),
                reachable(&[Opcode::Echo], &[3]),
                reachable(&[Opcode::Echo], &[4]),
                reachable(&[Opcode::Return], &[]),
            ],
            vec![],
        );
        assert_eq!(
            edges(&analysis),
            [
                (0, 1, EdgeKind::Engine),
                (0, 2, EdgeKind::SwitchCase(0)),
                (0, 3, EdgeKind::SwitchCase(1)),
                (0, 4, EdgeKind::SwitchDefault),
                (1, 4, EdgeKind::Flow),
                (2, 3, EdgeKind::Flow),
                (3, 4, EdgeKind::Flow),
            ],
            "SWITCH_LONG successors"
        );
    }

    #[test]
    fn match_jumptable_without_default_reports_no_arm() {
        let analysis = analyze(
            vec![
                entry(&[Opcode::Match], &[2, 3, 1]),
                reachable(&[Opcode::MatchError], &[]),
                reachable(&[Opcode::QmAssign, Opcode::Jmp], &[4]),
                reachable(&[Opcode::QmAssign, Opcode::Jmp], &[4]),
                reachable(&[Opcode::Return], &[]),
            ],
            vec![],
        );
        assert_eq!(
            edges(&analysis)[..3],
            [
                (0, 1, EdgeKind::MatchNoArm),
                (0, 2, EdgeKind::MatchArm(0)),
                (0, 3, EdgeKind::MatchArm(1))
            ],
            "MATCH successors"
        );
    }

    #[test]
    fn unknown_opcodes_are_reported_and_their_branches_fall_back_to_jump() {
        let analysis = analyze(
            vec![
                entry(
                    &[Opcode::Recv, Opcode::Unknown(240), Opcode::Unknown(241)],
                    &[2, 1],
                ),
                reachable(&[Opcode::Echo], &[2]),
                reachable(&[Opcode::Return], &[]),
            ],
            vec![],
        );
        assert_eq!(
            analysis.diagnostics,
            [
                Diagnostic::UnknownOpcode {
                    block: BlockId::new(0),
                    op: OpIndex::new(1),
                    raw: 240
                },
                Diagnostic::UnknownOpcode {
                    block: BlockId::new(0),
                    op: OpIndex::new(2),
                    raw: 241
                },
                Diagnostic::UnexpectedSuccessorCount {
                    block: BlockId::new(0),
                    opcode: Opcode::Unknown(241),
                    count: 2
                },
            ],
            "every unknown opcode and the unclassifiable branch are surfaced"
        );
        assert_eq!(
            edges(&analysis),
            [
                (0, 1, EdgeKind::Jump),
                (0, 2, EdgeKind::Jump),
                (1, 2, EdgeKind::Flow)
            ],
            "unclassifiable successors become Jump"
        );
    }

    #[test]
    fn unknown_fallthrough_opcode_still_flows() {
        let analysis = analyze(
            vec![
                entry(&[Opcode::Unknown(230)], &[1]),
                reachable(&[Opcode::Return], &[]),
            ],
            vec![],
        );
        assert_eq!(
            edges(&analysis),
            [(0, 1, EdgeKind::Flow)],
            "single fallthrough successor"
        );
        assert_eq!(
            analysis.diagnostics,
            [Diagnostic::UnknownOpcode {
                block: BlockId::new(0),
                op: OpIndex::new(0),
                raw: 230
            }],
            "the unknown opcode is still reported"
        );
    }

    #[test]
    fn arm_chain_without_default_jump_is_reported() {
        let analysis = analyze(
            vec![
                entry(&[Opcode::IsIdentical, Opcode::Jmpnz], &[2, 1]),
                reachable(&[Opcode::Echo], &[2]),
                reachable(&[Opcode::Return], &[]),
            ],
            vec![],
        );
        assert_eq!(
            analysis.diagnostics,
            [Diagnostic::UnrecognizedArmChain {
                block: BlockId::new(0)
            }],
            "a forward JMPNZ outside a match/switch chain is surfaced"
        );
        assert_eq!(
            edges(&analysis),
            [
                (0, 1, EdgeKind::Jump),
                (0, 2, EdgeKind::Jump),
                (1, 2, EdgeKind::Flow)
            ],
            "its successors become Jump"
        );
    }

    #[test]
    fn switch_true_chains_with_mixed_conditions_are_switches() {
        let analysis = analyze(
            vec![
                entry(&[Opcode::IsSmaller, Opcode::Jmpnz], &[3, 1]),
                reachable(&[Opcode::IsIdentical, Opcode::Jmpnz], &[4, 2]),
                reachable(&[Opcode::Jmp], &[5]),
                reachable(&[Opcode::Echo], &[4]),
                reachable(&[Opcode::Echo, Opcode::Jmp], &[5]),
                reachable(&[Opcode::Return], &[]),
            ],
            vec![],
        );
        assert!(
            analysis.diagnostics.is_empty(),
            "recognized chain: {:?}",
            analysis.diagnostics
        );
        assert_eq!(
            edges(&analysis)[..5],
            [
                (0, 1, EdgeKind::Flow),
                (0, 3, EdgeKind::SwitchCase(0)),
                (1, 2, EdgeKind::Flow),
                (1, 4, EdgeKind::SwitchCase(1)),
                (2, 5, EdgeKind::SwitchDefault),
            ],
            "switch (true) cases"
        );
    }

    #[test]
    fn loop_without_conditional_test_is_unstructured() {
        let analysis = analyze(
            vec![
                entry(&[Opcode::Recv], &[1]),
                reachable(&[Opcode::Echo, Opcode::Jmpz], &[3, 2]),
                reachable(&[Opcode::Echo, Opcode::Jmp], &[1]),
                reachable(&[Opcode::Return], &[]),
            ],
            vec![LoopInput {
                header: BlockId::new(1),
                body_blocks: body(&[2]),
            }],
        );
        assert_eq!(
            analysis.function.loops()[0].kind(),
            LoopShape::Unstructured,
            "goto-style loop"
        );
        assert_eq!(
            analysis.diagnostics,
            [Diagnostic::UnstructuredLoop {
                header: BlockId::new(1)
            }],
            "the unclassified loop is surfaced"
        );
        assert_eq!(
            edges(&analysis)[1..3],
            [(1, 2, EdgeKind::IfTrue), (1, 3, EdgeKind::IfFalse)],
            "a test that is not the loop's last block keeps its if kinds"
        );
    }

    #[test]
    fn unreachable_blocks_have_no_edges() {
        let mut dead = reachable(&[Opcode::Echo], &[2]);
        dead.flags = BlockFlags::empty();
        let analysis = analyze(
            vec![
                entry(&[Opcode::Return], &[]),
                dead,
                reachable(&[Opcode::Return], &[]),
            ],
            vec![],
        );
        assert!(
            edges(&analysis).is_empty(),
            "no edge leaves unreachable code"
        );
        assert!(
            !analysis.function.blocks()[1].is_reachable(),
            "the block is kept and marked unreachable"
        );
    }

    #[test]
    fn malformed_inputs_are_rejected_precisely() {
        assert_eq!(rejection(vec![], vec![]), CfgError::NoBlocks, "no blocks");
        assert_eq!(
            rejection(vec![entry(&[], &[])], vec![]),
            CfgError::EmptyBlock {
                block: BlockId::new(0)
            },
            "empty block"
        );
        assert_eq!(
            rejection(vec![entry(&[Opcode::Jmp], &[7])], vec![]),
            CfgError::EdgeEndpointMissing {
                from: BlockId::new(0),
                to: BlockId::new(7)
            },
            "successor out of range"
        );
        assert_eq!(
            rejection(vec![reachable(&[Opcode::Return], &[])], vec![]),
            CfgError::StartFlagMissing,
            "entry block without Start"
        );
        assert_eq!(
            rejection(
                vec![entry(&[Opcode::Echo], &[1]), entry(&[Opcode::Return], &[])],
                vec![]
            ),
            CfgError::StartFlagMisplaced {
                block: BlockId::new(1)
            },
            "second block with Start"
        );
        let two_blocks = || {
            return vec![
                entry(&[Opcode::Jmp], &[1]),
                reachable(&[Opcode::Return], &[]),
            ];
        };
        assert_eq!(
            rejection(
                two_blocks(),
                vec![LoopInput {
                    header: BlockId::new(1),
                    body_blocks: body(&[5])
                }]
            ),
            CfgError::LoopBlockMissing {
                header: BlockId::new(1),
                block: BlockId::new(5)
            },
            "loop body block out of range"
        );
        assert_eq!(
            rejection(
                two_blocks(),
                vec![LoopInput {
                    header: BlockId::new(1),
                    body_blocks: body(&[1])
                }]
            ),
            CfgError::LoopHeaderInBody {
                header: BlockId::new(1)
            },
            "header listed in its own body"
        );
        let looped = LoopInput {
            header: BlockId::new(1),
            body_blocks: body(&[]),
        };
        assert_eq!(
            rejection(two_blocks(), vec![looped.clone(), looped]),
            CfgError::DuplicateLoopHeader {
                header: BlockId::new(1)
            },
            "two loops with one header"
        );
        let mut inverted = function(two_blocks(), vec![]);
        inverted.line_end = line(1);
        assert_eq!(
            Analysis::try_from(inverted)
                .expect_err("inverted lines")
                .error(),
            CfgError::InvertedFunctionLines {
                line_start: line(2),
                line_end: line(1)
            },
            "function lines"
        );
    }

    #[test]
    fn snapshot_errors_name_the_function() {
        let error = Analysis::try_from(function(vec![], vec![])).expect_err("no blocks");
        assert_eq!(error.key().to_string(), "App\\f", "error carries the key");
        assert_eq!(
            error.to_string(),
            "cannot build the CFG snapshot of App\\f: the function has no basic blocks",
            "error message"
        );
    }

    fn valid_json() -> serde_json::Value {
        let analysis = analyze(
            vec![
                entry(&[Opcode::Jmpz], &[2, 1]),
                reachable(&[Opcode::Echo], &[2]),
                reachable(&[Opcode::Return], &[]),
            ],
            vec![],
        );
        return serde_json::to_value(&analysis.function).expect("serializes");
    }

    fn deserialization_error(json: serde_json::Value) -> String {
        return serde_json::from_value::<Function>(json)
            .expect_err("invalid snapshot")
            .to_string();
    }

    #[test]
    fn deserialized_snapshots_are_validated() {
        let valid = valid_json();
        serde_json::from_value::<Function>(valid.clone()).expect("the serialized form is accepted");

        let mut gap = valid.clone();
        gap["blocks"][1]["op_start"] = 2.into();
        gap["blocks"][1]["op_end"] = 2.into();
        assert!(
            deserialization_error(gap).contains("starts at opcode 0002 instead of 0001"),
            "op gap"
        );

        let mut range = valid.clone();
        range["blocks"][0]["op_end"] = 3.into();
        assert!(
            deserialization_error(range).contains("spans opcodes"),
            "op range"
        );

        let mut lines = valid.clone();
        lines["blocks"][0]["line_end"] = 4.into();
        assert!(
            deserialization_error(lines).contains("differ from its opcodes' lines"),
            "line range"
        );

        let mut unsorted = valid.clone();
        unsorted["edges"].as_array_mut().expect("edges").reverse();
        assert!(
            deserialization_error(unsorted).contains("out of (from, to, kind) order"),
            "edge order"
        );

        let mut duplicate = valid.clone();
        let first = duplicate["edges"][0].clone();
        duplicate["edges"]
            .as_array_mut()
            .expect("edges")
            .insert(0, first);
        assert!(
            deserialization_error(duplicate).contains("listed twice"),
            "duplicate edge"
        );

        let mut unreachable = valid.clone();
        unreachable["blocks"][1]["flags"] = serde_json::json!([]);
        assert!(
            deserialization_error(unreachable).contains("unreachable"),
            "edge from unreachable block"
        );

        let mut zero = valid.clone();
        zero["line_start"] = 0.into();
        serde_json::from_value::<Function>(zero).expect_err("line zero is rejected");

        let mut flags = valid;
        flags["blocks"][0]["flags"] = serde_json::json!(["Reachable", "Start"]);
        assert!(
            deserialization_error(flags).contains("must be unique and listed in the order"),
            "flag order"
        );
    }

    #[test]
    fn engine_flag_bits_map_to_typed_flags() {
        let block = BlockFlags::from_zend_block_flags(
            ZEND_BB_START | ZEND_BB_TARGET | ZEND_BB_CATCH | ZEND_BB_REACHABLE,
        );
        assert_eq!(
            block.iter().collect::<Vec<_>>(),
            [BlockFlag::Start, BlockFlag::Catch, BlockFlag::Reachable],
            "ZEND_BB_* bits"
        );
        assert_eq!(
            BlockFlags::from_zend_block_flags(ZEND_BB_FINALLY | ZEND_BB_FINALLY_END)
                .iter()
                .collect::<Vec<_>>(),
            [BlockFlag::Finally],
            "ZEND_BB_FINALLY without FINALLY_END noise"
        );
        assert_eq!(
            FunctionFlags::from_zend_fn_flags(ZEND_ACC_STATIC),
            FunctionFlags::empty(),
            "static method"
        );
        assert_eq!(
            FunctionFlags::from_zend_fn_flags(ZEND_ACC_STATIC | ZEND_ACC_CLOSURE)
                .iter()
                .collect::<Vec<_>>(),
            [FunctionFlag::StaticClosure],
            "static closure"
        );
        assert_eq!(
            FunctionFlags::from_zend_fn_flags(
                ZEND_ACC_GENERATOR
                    | ZEND_ACC_RETURN_REFERENCE
                    | ZEND_ACC_ABSTRACT
                    | ZEND_ACC_CLOSURE,
            )
            .iter()
            .collect::<Vec<_>>(),
            [
                FunctionFlag::Generator,
                FunctionFlag::Abstract,
                FunctionFlag::ReturnsReference
            ],
            "generator, abstract, by-reference return"
        );
    }
}
