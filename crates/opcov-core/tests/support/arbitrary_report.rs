#![expect(
    clippy::redundant_pub_crate,
    reason = "pub(super) is the intended visibility; the pub this lint suggests is denied by rustc unreachable_pub"
)]

use std::collections::{BTreeMap, BTreeSet};

use opcov_core::cfg::{FunctionKey, Line, LoopShape, OpIndex};
use opcov_core::kind::EdgeKind;
use opcov_core::report::{
    FileCoverage, Function, Generator, Header, HitSet, IdSorted, Loop, LoopId, MeasuredShape,
    Outcome, OutcomeId, OutcomeKind, PathBase, Report, Root, Settings, Sha256, SourcePath, Span,
    TestIndex, TestTable, VmKind,
};
use proptest::prelude::*;

pub(super) const UNIT_OUTCOME_KINDS: [EdgeKind; 21] = [
    EdgeKind::IfTrue,
    EdgeKind::IfFalse,
    EdgeKind::ShortCircuit,
    EdgeKind::Evaluate,
    EdgeKind::CoalesceValue,
    EdgeKind::CoalesceFallback,
    EdgeKind::TernaryValue,
    EdgeKind::TernaryFallback,
    EdgeKind::NullsafeNull,
    EdgeKind::NullsafeValue,
    EdgeKind::MatchNoArm,
    EdgeKind::SwitchDefault,
    EdgeKind::LoopEnter,
    EdgeKind::LoopSkip,
    EdgeKind::LoopBack,
    EdgeKind::LoopExit,
    EdgeKind::Catch,
    EdgeKind::Finally,
    EdgeKind::FinallyException,
    EdgeKind::Engine,
    EdgeKind::Jump,
];

const LOOP_SHAPES: [LoopShape; 3] = [LoopShape::Foreach, LoopShape::PreTest, LoopShape::PostTest];

#[derive(Debug, Clone)]
struct FunctionModel {
    span: Option<Span>,
    outcomes: BTreeMap<u32, (OutcomeKind, Line, Option<Span>)>,
    loops: BTreeMap<u32, (MeasuredShape, Line, bool)>,
}

#[derive(Debug, Clone)]
struct FileModel {
    sha256: Sha256,
    lines: BTreeSet<u32>,
    functions: BTreeMap<String, FunctionModel>,
}

#[derive(Debug, Clone)]
pub(super) struct Universe {
    tests: Vec<String>,
    files: BTreeMap<String, FileModel>,
}

#[derive(Debug, Clone)]
pub(super) struct Run {
    pub(super) tests: u8,
    pub(super) files: u8,
    pub(super) pattern: Vec<u8>,
}

fn outcome_kind() -> impl Strategy<Value = OutcomeKind> {
    return prop_oneof![
        prop::sample::select(UNIT_OUTCOME_KINDS.to_vec()),
        (0_u32..4).prop_map(EdgeKind::MatchArm),
        (0_u32..4).prop_map(EdgeKind::SwitchCase),
    ]
    .prop_map(|kind| return OutcomeKind::new(kind).unwrap());
}

fn span_at(line: u32) -> impl Strategy<Value = Span> {
    return (1_u32..80, 0_u32..3, 1_u32..20).prop_map(move |(column, extra, width)| {
        let end_column = if extra == 0 {
            column.saturating_add(width)
        } else {
            width
        };
        return Span::new([line, column, line.saturating_add(extra), end_column]).unwrap();
    });
}

fn line_and_span() -> impl Strategy<Value = (Line, Option<Span>)> {
    return (1_u32..300).prop_flat_map(|line| {
        return (
            Just(Line::new(line).unwrap()),
            prop::option::of(span_at(line)),
        );
    });
}

fn function_model() -> impl Strategy<Value = FunctionModel> {
    let outcome = (outcome_kind(), line_and_span())
        .prop_map(|(kind, (line, span))| return (kind, line, span));
    let looped = (
        prop::sample::select(LOOP_SHAPES.to_vec())
            .prop_map(|shape| return MeasuredShape::new(shape).unwrap()),
        (1_u32..300).prop_map(|line| return Line::new(line).unwrap()),
        any::<bool>(),
    );
    return (
        prop::option::of((1_u32..300).prop_flat_map(span_at)),
        prop::collection::btree_map(0_u32..20, outcome, 0..5),
        prop::collection::btree_map(0_u32..5, looped, 0..3),
    )
        .prop_map(|(span, outcomes, loops)| {
            return FunctionModel {
                span,
                outcomes,
                loops,
            };
        });
}

fn file_model() -> impl Strategy<Value = FileModel> {
    return (
        any::<[u8; 32]>(),
        prop::collection::btree_set(1_u32..300, 0..6),
        prop::collection::btree_map(
            (prop::sample::select(vec!["C", "C%E9", "\u{e9}"]), 0_u8..6)
                .prop_map(|(class, number)| return format!("App\\{class}{number}::run")),
            function_model(),
            0..4,
        ),
    )
        .prop_map(|(sha256, lines, functions)| {
            return FileModel {
                sha256: Sha256::from_bytes(sha256),
                lines,
                functions,
            };
        });
}

pub(super) fn universe() -> impl Strategy<Value = Universe> {
    let tests = prop::collection::btree_set(0_u8..16, 0..=8).prop_map(|numbers| {
        return numbers
            .into_iter()
            .map(|number| return format!("Tests\\Case{number:02}Test::test_it"))
            .collect::<Vec<_>>();
    });
    let files = prop::collection::btree_map(
        (
            prop::sample::select(vec!["d0", "d1", "d%E9", "d%25"]),
            0_u8..10,
        )
            .prop_map(|(dir, number)| return format!("src/{dir}/F{number}.php")),
        file_model(),
        0..=5,
    );
    return (tests, files).prop_map(|(tests, files)| return Universe { tests, files });
}

pub(super) fn run() -> impl Strategy<Value = Run> {
    return (
        any::<u8>(),
        any::<u8>(),
        prop::collection::vec(any::<u8>(), 1..16),
    )
        .prop_map(|(tests, files, pattern)| {
            return Run {
                tests,
                files,
                pattern,
            };
        });
}

fn bit(mask: u8, position: usize) -> bool {
    return u32::try_from(position)
        .ok()
        .and_then(|shift| return 1_u8.checked_shl(shift))
        .is_some_and(|flag| return mask & flag != 0);
}

struct Hits<'a> {
    pattern: std::iter::Cycle<std::iter::Copied<std::slice::Iter<'a, u8>>>,
    rotation: u32,
    selected: u8,
    local: &'a [Option<TestIndex>],
}

impl Hits<'_> {
    fn draw(&mut self) -> HitSet {
        let mask = self.pattern.next().unwrap().rotate_left(self.rotation) & self.selected;
        self.rotation = self.rotation.wrapping_add(1);
        let indexes = self
            .local
            .iter()
            .enumerate()
            .filter(|(position, _)| return bit(mask, *position))
            .filter_map(|(_, index)| return *index)
            .collect();
        return HitSet::new(indexes).unwrap();
    }
}

pub(super) fn header(tests: Vec<String>) -> Header {
    return Header {
        generator: Generator {
            opcov: "1.0.0".to_owned(),
            php: "8.5.12".to_owned(),
            zts: true,
            vm: VmKind::Hybrid,
        },
        settings: Settings {
            collect_paths: BTreeSet::from(["src".to_owned()]),
            exclude_paths: BTreeSet::from(["src/generated".to_owned()]),
            exclude_pattern: Some("~Stub\\.php$~".to_owned()),
        },
        paths: PathBase::Relative(Root::new("/srv/app".to_owned()).unwrap()),
        tests: TestTable::new(tests).unwrap(),
    };
}

impl Universe {
    pub(super) fn report(&self, run: &Run) -> Report {
        let mut tests = Vec::new();
        let mut local = Vec::with_capacity(self.tests.len());
        for (position, name) in self.tests.iter().enumerate() {
            if bit(run.tests, position) {
                local.push(Some(TestIndex::new(u32::try_from(tests.len()).unwrap())));
                tests.push(name.clone());
            } else {
                local.push(None);
            }
        }
        let mut hits = Hits {
            pattern: run.pattern.iter().copied().cycle(),
            rotation: 0,
            selected: run.tests,
            local: &local,
        };
        let mut files = BTreeMap::new();
        for (position, (path, model)) in self.files.iter().enumerate() {
            let lines = model
                .lines
                .iter()
                .map(|line| return (Line::new(*line).unwrap(), hits.draw()))
                .collect();
            let functions = model
                .functions
                .iter()
                .map(|(key, function)| {
                    let outcomes = function
                        .outcomes
                        .iter()
                        .map(|(id, (kind, line, span))| {
                            return Outcome::new(
                                OutcomeId::new(*id),
                                OpIndex::new(*id),
                                *kind,
                                *line,
                                *span,
                                hits.draw(),
                            )
                            .unwrap();
                        })
                        .collect();
                    let loops = function
                        .loops
                        .iter()
                        .map(|(id, (kind, line, zero_possible))| {
                            let zero = hits.draw();
                            return Loop::new(
                                LoopId::new(*id),
                                OpIndex::new(id.saturating_add(1000)),
                                *kind,
                                *line,
                                zero_possible.then_some(zero),
                                hits.draw(),
                            );
                        })
                        .collect();
                    let function = Function {
                        span: function.span,
                        outcomes: IdSorted::new(outcomes).unwrap(),
                        loops: IdSorted::new(loops).unwrap(),
                    };
                    return (key.parse::<FunctionKey>().unwrap(), function);
                })
                .collect();
            if bit(run.files, position) {
                files.insert(
                    SourcePath::new(path.clone()).unwrap(),
                    FileCoverage {
                        sha256: model.sha256,
                        lines,
                        functions,
                    },
                );
            }
        }
        return Report::new(header(tests), files).unwrap();
    }
}

pub(super) fn bytes(report: &Report) -> Vec<u8> {
    return report.write(Vec::new()).unwrap();
}
