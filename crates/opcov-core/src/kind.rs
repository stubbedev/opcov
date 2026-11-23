use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub enum EdgeKind {
    Flow,
    IfTrue,
    IfFalse,
    ShortCircuit,
    Evaluate,
    CoalesceValue,
    CoalesceFallback,
    TernaryValue,
    TernaryFallback,
    NullsafeNull,
    NullsafeValue,
    MatchArm(u32),
    MatchNoArm,
    SwitchCase(u32),
    SwitchDefault,
    LoopEnter,
    LoopSkip,
    LoopBack,
    LoopExit,
    Catch,
    Finally,
    Engine,
    Jump,
}

impl EdgeKind {
    #[must_use]
    pub const fn is_outcome(self) -> bool {
        return !matches!(self, Self::Flow);
    }

    #[must_use]
    pub const fn counts_toward_verdict(self) -> bool {
        return !matches!(self, Self::Flow | Self::Engine);
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub enum LoopKind {
    Foreach,
    For,
    While,
    DoWhile,
}
