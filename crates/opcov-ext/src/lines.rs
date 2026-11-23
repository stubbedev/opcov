use std::ops::Range;

use opcov_core::cfg::{Line, OpIndex, OpSpan, Opcode, PhpVersion};
use opcov_sys::{IS_CONST, IS_CV, IS_TMP_VAR, IS_VAR};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct OperandType(u8);

impl OperandType {
    pub(crate) const fn new(raw: u8) -> Self {
        return Self(raw);
    }

    const fn is_const(self) -> bool {
        return self.0 == IS_CONST;
    }

    const fn is_const_or_cv(self) -> bool {
        return self.0 & (IS_CONST | IS_CV) != 0;
    }

    const fn is_tmp_or_var(self) -> bool {
        return self.0 & (IS_TMP_VAR | IS_VAR) != 0;
    }

    const fn is_exactly_cv(self) -> bool {
        return self.0 == IS_CV;
    }

    const fn either(self, other: Self) -> Self {
        return Self(self.0 | other.0);
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Instr {
    pub(crate) opcode: Opcode,
    pub(crate) op1: OperandType,
    pub(crate) op2: OperandType,
    pub(crate) result: OperandType,
    pub(crate) line: Line,
}

impl Instr {
    const fn operands(self) -> OperandType {
        return self.op1.either(self.op2);
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct LineBlock {
    pub(crate) reachable: bool,
    pub(crate) protected_entry: bool,
    pub(crate) falls_from_previous: bool,
    pub(crate) ops: Range<usize>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct ProbeSite {
    pub(crate) op: usize,
    pub(crate) line: Line,
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub(crate) struct Placement {
    pub(crate) probes: Vec<ProbeSite>,
    pub(crate) lines: Vec<Line>,
}

pub(crate) const fn is_ignored(opcode: Opcode) -> bool {
    return matches!(
        opcode,
        Opcode::Nop
            | Opcode::OpData
            | Opcode::FeFree
            | Opcode::Free
            | Opcode::AssertCheck
            | Opcode::VerifyReturnType
            | Opcode::Recv
            | Opcode::RecvInit
            | Opcode::RecvVariadic
            | Opcode::SendVal
            | Opcode::SendVarEx
            | Opcode::SendRef
            | Opcode::SendUnpack
            | Opcode::DeclareConst
            | Opcode::DeclareClass
            | Opcode::DeclareClassDelayed
            | Opcode::DeclareFunction
            | Opcode::DeclareAnonClass
            | Opcode::FastRet
            | Opcode::FastCall
            | Opcode::Ticks
            | Opcode::ExtStmt
            | Opcode::ExtFcallBegin
            | Opcode::ExtFcallEnd
            | Opcode::ExtNop
            | Opcode::BindGlobal
    );
}

const fn is_recv(opcode: Opcode) -> bool {
    return matches!(
        opcode,
        Opcode::Recv | Opcode::RecvInit | Opcode::RecvVariadic
    );
}

const fn sends_argument(opcode: Opcode) -> bool {
    return matches!(
        opcode,
        Opcode::SendVal
            | Opcode::SendValEx
            | Opcode::SendVar
            | Opcode::SendVarEx
            | Opcode::SendRef
            | Opcode::SendVarNoRef
            | Opcode::SendVarNoRefEx
            | Opcode::SendFuncArg
            | Opcode::SendUnpack
            | Opcode::SendArray
            | Opcode::SendUser
    );
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ConsumedOperand {
    First,
    Second,
}

fn consumed_operand(version: PhpVersion, opcode: Opcode) -> Option<ConsumedOperand> {
    return match opcode {
        Opcode::Jmpz
        | Opcode::Jmpnz
        | Opcode::JmpzEx
        | Opcode::JmpnzEx
        | Opcode::JmpSet
        | Opcode::Coalesce
        | Opcode::JmpNull
        | Opcode::BwNot
        | Opcode::Cast
        | Opcode::QmAssign
        | Opcode::Echo
        | Opcode::Strlen
        | Opcode::Count
        | Opcode::Return
        | Opcode::ReturnByRef
        | Opcode::GeneratorReturn
        | Opcode::InitArray
        | Opcode::AddArrayElement
        | Opcode::AddArrayUnpack
        | Opcode::FetchListR
        | Opcode::SendValEx
        | Opcode::SendVarNoRef
        | Opcode::SendVarNoRefEx => Some(ConsumedOperand::First),
        Opcode::Bool | Opcode::BoolNot if version >= PhpVersion::Php85 => {
            Some(ConsumedOperand::First)
        }
        Opcode::Add
        | Opcode::Sub
        | Opcode::Div
        | Opcode::Mod
        | Opcode::Sl
        | Opcode::Sr
        | Opcode::Concat
        | Opcode::Pow => Some(ConsumedOperand::Second),
        _ => None,
    };
}

fn consumed_type(version: PhpVersion, op: Instr) -> Option<OperandType> {
    return consumed_operand(version, op.opcode).map(|operand| {
        return match operand {
            ConsumedOperand::First => op.op1,
            ConsumedOperand::Second => op.op2,
        };
    });
}

fn line_has_only_consumers(version: PhpVersion, ops: &[Instr]) -> bool {
    let Some(first) = ops.first() else {
        return false;
    };
    let line = first.line;
    let mut open_result = false;
    for (position, &op) in ops.iter().enumerate() {
        if is_ignored(op.opcode) {
            if op.line == line && op.operands().is_const_or_cv() {
                return false;
            }
            open_result = false;
            continue;
        }
        if op.line != line {
            break;
        }
        if op.opcode == Opcode::Jmp {
            if !open_result {
                return false;
            }
            open_result = false;
            continue;
        }
        if !consumed_type(version, op).is_some_and(OperandType::is_tmp_or_var) {
            return false;
        }
        let next_sends_variable = position
            .checked_add(1)
            .and_then(|next| return ops.get(next))
            .is_some_and(|next| {
                return sends_argument(next.opcode) && next.op1.is_exactly_cv();
            });
        if sends_argument(op.opcode) && next_sends_variable {
            return false;
        }
        open_result = op.result.is_tmp_or_var();
    }
    return true;
}

fn block_ops<'a>(instrs: &'a [Instr], block: &LineBlock) -> &'a [Instr] {
    return instrs.get(block.ops.clone()).unwrap_or_default();
}

pub(crate) fn declarations_only(instrs: &[Instr]) -> bool {
    return instrs.split_last().is_some_and(|(last, rest)| {
        return last.opcode == Opcode::Return
            && last.op1.is_const()
            && rest.iter().all(|op| return is_ignored(op.opcode));
    });
}

fn in_fallback(fallbacks: &[OpSpan], op: usize) -> bool {
    return u32::try_from(op).is_ok_and(|op| {
        return fallbacks
            .iter()
            .any(|span| return span.contains(OpIndex::new(op)));
    });
}

pub(crate) fn place(
    version: PhpVersion,
    instrs: &[Instr],
    blocks: &[LineBlock],
    fallbacks: &[OpSpan],
) -> Placement {
    let mut placement = Placement::default();
    let mut last_line: Option<Line> = None;
    for block in blocks {
        if !block.reachable {
            last_line = None;
            continue;
        }
        let mut operand_line = if block.protected_entry || !block.falls_from_previous {
            None
        } else {
            last_line
        };
        last_line = None;
        let ops = block_ops(instrs, block);
        let mut block_entry = true;
        let mut probed_line: Option<Line> = None;
        let mut consumers = false;
        let mut position = 0_usize;
        while let Some(&op) = ops.get(position) {
            let rest = ops.get(position..).unwrap_or_default();
            let index = block.ops.start.saturating_add(position);
            position = position.saturating_add(1);
            if is_ignored(op.opcode) {
                if is_recv(op.opcode) {
                    block_entry = true;
                }
                if op.operands().is_const_or_cv() {
                    operand_line = None;
                }
                continue;
            }
            if placement.lines.last() != Some(&op.line) && !in_fallback(fallbacks, index) {
                placement.lines.push(op.line);
            }
            last_line = Some(op.line);
            let new_line = block_entry || Some(op.line) != probed_line;
            if new_line {
                consumers = Some(op.line) == operand_line && line_has_only_consumers(version, rest);
            }
            if !consumers && op.operands().is_const_or_cv() {
                operand_line = None;
            }
            if !consumers && new_line {
                placement.probes.push(ProbeSite {
                    op: index,
                    line: op.line,
                });
            }
            if consumers || new_line {
                block_entry = false;
                probed_line = Some(op.line);
            }
            let constructor_follows = ops
                .get(position)
                .is_some_and(|next| return next.opcode == Opcode::DoFcall);
            if op.opcode == Opcode::New && constructor_follows {
                position = position.saturating_add(1);
            }
        }
    }
    placement.lines.sort_unstable();
    placement.lines.dedup();
    if !fallbacks.is_empty() {
        let Placement { probes, lines } = &mut placement;
        probes.retain(|probe| {
            return !in_fallback(fallbacks, probe.op) || lines.binary_search(&probe.line).is_ok();
        });
    }
    return placement;
}

#[cfg(test)]
mod tests {
    use super::*;

    const UNUSED: OperandType = OperandType::new(opcov_sys::IS_UNUSED);
    const CONST: OperandType = OperandType::new(IS_CONST);
    const TMP: OperandType = OperandType::new(IS_TMP_VAR);
    const CV: OperandType = OperandType::new(IS_CV);

    fn line(number: u32) -> Line {
        return Line::new(number).unwrap();
    }

    fn op(opcode: Opcode, number: u32) -> Instr {
        return Instr {
            opcode,
            op1: UNUSED,
            op2: UNUSED,
            result: UNUSED,
            line: line(number),
        };
    }

    fn with(instr: Instr, op1: OperandType, op2: OperandType, result: OperandType) -> Instr {
        return Instr {
            op1,
            op2,
            result,
            ..instr
        };
    }

    fn block(ops: Range<usize>, falls_from_previous: bool) -> LineBlock {
        return LineBlock {
            reachable: true,
            protected_entry: ops.start == 0,
            falls_from_previous,
            ops,
        };
    }

    fn probe_lines(placement: &Placement) -> Vec<(usize, u32)> {
        return placement
            .probes
            .iter()
            .map(|site| return (site.op, site.line.get()))
            .collect();
    }

    #[test]
    fn one_probe_per_line_and_ignored_opcodes_are_not_executable() {
        let instrs = [
            op(Opcode::Recv, 2),
            with(op(Opcode::Assign, 3), CV, CONST, UNUSED),
            with(op(Opcode::Echo, 3), CV, UNUSED, UNUSED),
            with(op(Opcode::Return, 4), CONST, UNUSED, UNUSED),
        ];
        let placement = place(PhpVersion::Php85, &instrs, &[block(0..4, false)], &[]);
        assert_eq!(probe_lines(&placement), vec![(1, 3), (3, 4)], "probes");
        assert_eq!(
            placement
                .lines
                .iter()
                .map(|l| return l.get())
                .collect::<Vec<_>>(),
            vec![3, 4],
            "RECV is not an executable line"
        );
    }

    #[test]
    fn a_frameless_fallback_copy_adds_no_line_and_probes_only_lines_executable_elsewhere() {
        let instrs = [
            with(op(Opcode::JmpFrameless, 4), CONST, UNUSED, UNUSED),
            op(Opcode::InitNsFcallByName, 4),
            with(op(Opcode::SendValEx, 4), CONST, UNUSED, UNUSED),
            with(op(Opcode::SendValEx, 5), CONST, UNUSED, UNUSED),
            op(Opcode::DoFcallByName, 4),
            op(Opcode::Jmp, 4),
            with(op(Opcode::FramelessIcall2, 4), CONST, CONST, TMP),
            with(op(Opcode::Return, 6), TMP, UNUSED, UNUSED),
        ];
        let blocks = [
            block(0..1, false),
            block(1..6, true),
            block(6..7, false),
            block(7..8, true),
        ];
        let fallback = OpSpan {
            start: OpIndex::new(1),
            end: OpIndex::new(6),
        };
        let placement = place(PhpVersion::Php85, &instrs, &blocks, &[fallback]);
        assert_eq!(
            probe_lines(&placement),
            vec![(0, 4), (1, 4), (4, 4), (6, 4), (7, 6)],
            "the fallback copy keeps only its probes on line 4"
        );
        assert_eq!(
            placement
                .lines
                .iter()
                .map(|l| return l.get())
                .collect::<Vec<_>>(),
            vec![4, 6],
            "line 5 exists only in the fallback copy"
        );
    }

    #[test]
    fn every_block_entry_gets_a_probe_even_on_the_same_line() {
        let instrs = [
            with(op(Opcode::Jmpz, 3), CV, UNUSED, UNUSED),
            with(op(Opcode::Echo, 3), CONST, UNUSED, UNUSED),
            with(op(Opcode::Return, 3), CONST, UNUSED, UNUSED),
        ];
        let blocks = [block(0..1, false), block(1..2, true), block(2..3, true)];
        let placement = place(PhpVersion::Php85, &instrs, &blocks, &[]);
        assert_eq!(
            probe_lines(&placement),
            vec![(0, 3), (1, 3), (2, 3)],
            "probes"
        );
    }

    #[test]
    fn a_consumer_on_the_operand_line_after_fallthrough_gets_no_probe() {
        let instrs = [
            with(op(Opcode::JmpzEx, 3), CV, UNUSED, TMP),
            with(op(Opcode::Bool, 4), CV, UNUSED, TMP),
            with(op(Opcode::Jmpz, 4), TMP, UNUSED, UNUSED),
            with(op(Opcode::Echo, 5), CONST, UNUSED, UNUSED),
        ];
        let blocks = [
            block(0..1, false),
            block(1..2, true),
            block(2..3, true),
            block(3..4, true),
        ];
        let placement = place(PhpVersion::Php85, &instrs, &blocks, &[]);
        assert_eq!(
            probe_lines(&placement),
            vec![(0, 3), (1, 4), (3, 5)],
            "probes"
        );
        assert_eq!(
            placement
                .lines
                .iter()
                .map(|l| return l.get())
                .collect::<Vec<_>>(),
            vec![3, 4, 5],
            "the consumer's line stays executable"
        );
    }

    #[test]
    fn new_and_its_constructor_call_count_once() {
        let instrs = [
            with(op(Opcode::New, 3), CONST, UNUSED, TMP),
            op(Opcode::DoFcall, 4),
            with(op(Opcode::Return, 5), CONST, UNUSED, UNUSED),
        ];
        let placement = place(PhpVersion::Php85, &instrs, &[block(0..3, false)], &[]);
        assert_eq!(probe_lines(&placement), vec![(0, 3), (2, 5)], "probes");
        assert!(
            !placement.lines.contains(&line(4)),
            "the constructor call's line is not executable"
        );
    }

    #[test]
    fn unreachable_blocks_are_ignored() {
        let instrs = [
            with(op(Opcode::Return, 3), CONST, UNUSED, UNUSED),
            with(op(Opcode::Return, 4), CONST, UNUSED, UNUSED),
        ];
        let blocks = [
            block(0..1, false),
            LineBlock {
                reachable: false,
                ..block(1..2, false)
            },
        ];
        let placement = place(PhpVersion::Php85, &instrs, &blocks, &[]);
        assert_eq!(probe_lines(&placement), vec![(0, 3)], "probes");
    }

    #[test]
    fn a_main_of_declarations_and_a_constant_return_is_declarations_only() {
        let declare = op(Opcode::DeclareClassDelayed, 3);
        let ret = with(op(Opcode::Return, 9), CONST, UNUSED, UNUSED);
        assert!(declarations_only(&[ret]), "a lone RETURN");
        assert!(
            declarations_only(&[declare, ret]),
            "declarations, then RETURN"
        );
        let echo = with(op(Opcode::Echo, 4), CONST, UNUSED, UNUSED);
        assert!(!declarations_only(&[echo, ret]), "executable code");
        let returns_variable = with(op(Opcode::Return, 9), CV, UNUSED, UNUSED);
        assert!(
            !declarations_only(&[returns_variable]),
            "a RETURN of a variable"
        );
    }

    #[test]
    fn bool_is_a_consumer_from_php_8_5_only() {
        let bool_op = with(op(Opcode::Bool, 4), TMP, UNUSED, TMP);
        assert_eq!(
            consumed_operand(PhpVersion::Php84, bool_op.opcode),
            None,
            "8.4"
        );
        assert_eq!(
            consumed_operand(PhpVersion::Php85, bool_op.opcode),
            Some(ConsumedOperand::First),
            "8.5"
        );
    }
}
