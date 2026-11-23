<?php

declare(strict_types=1);

namespace Opcov\Measure\Gate;

enum Category: string
{
    case FunctionOnlyOpcov = 'function-only-opcov';
    case FunctionOnlyXdebug = 'function-only-xdebug';
    case FileOnlyXdebug = 'file-only-xdebug';
    case LineBoth = 'line-both';
    case LineXdebugOnlyCoveredByOpcov = 'line-xdebug-only-covered-by-opcov';
    case LineXdebugOnlyNotExecutableForOpcov = 'line-xdebug-only-not-executable-for-opcov';
    case LineOpcovOnlyCoveredByXdebug = 'line-opcov-only-covered-by-xdebug';
    case LineOpcovOnlyNotExecutableForXdebug = 'line-opcov-only-not-executable-for-xdebug';
    case BranchOnUncoveredLine = 'branch-on-uncovered-line';
    case BranchUnseenOutcome = 'branch-unseen-outcome';
    case BranchBehindUnexecutedBranch = 'branch-behind-unexecuted-branch';
    case BranchExcludedOutcome = 'branch-excluded-outcome';
    case BranchUnmatched = 'branch-unmatched';
    case OutcomeOnUncoveredLine = 'outcome-on-uncovered-line';
    case OutcomeUnexecutedBranch = 'outcome-unexecuted-branch';
    case OutcomeInFramelessFallback = 'outcome-in-frameless-fallback';
    case OutcomeSplit = 'outcome-split';
    case OutcomeEdge = 'outcome-edge';
    case OutcomeUnclassified = 'outcome-unclassified';
    case LoopZero = 'loop-zero';
    case LoopSecond = 'loop-second';

    public function side(): Side
    {
        return match ($this) {
            self::LineBoth, self::BranchOnUncoveredLine, self::BranchUnseenOutcome, self::BranchBehindUnexecutedBranch, self::OutcomeOnUncoveredLine, self::OutcomeUnexecutedBranch => Side::Both,
            self::FunctionOnlyOpcov, self::LineOpcovOnlyCoveredByXdebug, self::OutcomeInFramelessFallback, self::LineOpcovOnlyNotExecutableForXdebug, self::OutcomeSplit, self::OutcomeEdge, self::OutcomeUnclassified, self::LoopZero, self::LoopSecond => Side::OpcovOnly,
            self::FunctionOnlyXdebug, self::FileOnlyXdebug, self::LineXdebugOnlyCoveredByOpcov, self::LineXdebugOnlyNotExecutableForOpcov, self::BranchExcludedOutcome, self::BranchUnmatched => Side::XdebugOnly,
        };
    }

    public function explainedByRule(): bool
    {
        return match ($this) {
            self::OutcomeSplit, self::OutcomeEdge, self::LoopZero, self::LoopSecond, self::BranchExcludedOutcome => true,
            default => $this->side() === Side::Both,
        };
    }

    public function description(): string
    {
        return match ($this) {
            self::FunctionOnlyOpcov => 'function selected by opcov only',
            self::FunctionOnlyXdebug => 'function selected by the Xdebug gate only',
            self::FileOnlyXdebug => 'changed file php-code-coverage reports and the opcov report lacks',
            self::LineBoth => 'uncovered line, both',
            self::LineXdebugOnlyCoveredByOpcov => 'line uncovered for Xdebug, covered for opcov',
            self::LineXdebugOnlyNotExecutableForOpcov => 'line uncovered for Xdebug, not executable for opcov',
            self::LineOpcovOnlyCoveredByXdebug => 'line uncovered for opcov, covered for Xdebug',
            self::LineOpcovOnlyNotExecutableForXdebug => 'line uncovered for opcov, not executable for Xdebug',
            self::BranchOnUncoveredLine => 'Xdebug branch never ran, opcov has an uncovered line in it',
            self::BranchUnseenOutcome => 'Xdebug branch never ran, opcov has an unseen outcome into it',
            self::BranchBehindUnexecutedBranch => 'Xdebug branch never ran, reached only from branches that never ran',
            self::BranchExcludedOutcome => 'Xdebug branch never ran, entered by an outcome opcov excludes from verdicts (`Engine`, `LoopEnter`/`LoopSkip`)',
            self::BranchUnmatched => 'Xdebug branch never ran, no opcov finding',
            self::OutcomeOnUncoveredLine => 'unseen outcome on a line no test ran',
            self::OutcomeUnexecutedBranch => 'unseen outcome into an Xdebug branch that never ran',
            self::OutcomeInFramelessFallback => 'unseen outcome of a construct copied into the never-taken `JMP_FRAMELESS` fallback of its line',
            self::OutcomeSplit => 'unseen `??`/`?:`/`?->` outcome (Xdebug has no branch)',
            self::OutcomeEdge => 'unseen outcome whose target block ran through another edge (Xdebug counts blocks)',
            self::OutcomeUnclassified => 'unseen outcome without an Xdebug counterpart',
            self::LoopZero => 'loop never ran zero times',
            self::LoopSecond => 'loop never ran two or more times',
        };
    }
}
