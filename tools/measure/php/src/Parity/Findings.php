<?php

declare(strict_types=1);

namespace Opcov\Measure\Parity;

final class Findings
{
    /** @var list<Divergence> */
    private array $divergences = [];

    /** @var list<Divergence> */
    private array $disagreements = [];

    /** @var array<string, int> */
    private array $compared = [];

    private int $anonymousDecisions = 0;

    private int $jumpTables = 0;

    private int $unobservedFiles = 0;

    private int $evaluatedEntries = 0;

    private int $expectationsChecked = 0;

    private int $unobservedFunctions = 0;

    private int $shiftedTargets = 0;

    private int $outcomesNotCompared = 0;

    private int $matchedDecisions = 0;

    private int $opcovOnlyDecisions = 0;

    private int $hitsCompared = 0;

    private int $hitsUnknown = 0;

    public function diverge(Divergence $divergence): void
    {
        $this->divergences[] = $divergence;
    }

    public function disagree(Divergence $disagreement): void
    {
        $this->disagreements[] = $disagreement;
    }

    public function anonymous(int $decisions): void
    {
        $this->anonymousDecisions += $decisions;
    }

    public function unobservedFunction(): void
    {
        $this->unobservedFunctions++;
    }

    public function outcomesNotCompared(): void
    {
        $this->outcomesNotCompared++;
    }

    public function shiftedTargets(): void
    {
        $this->shiftedTargets++;
    }

    public function expectationChecked(): void
    {
        $this->expectationsChecked++;
    }

    public function unobserved(): void
    {
        $this->unobservedFiles++;
    }

    public function evaluated(): void
    {
        $this->evaluatedEntries++;
    }

    public function jumpTable(): void
    {
        $this->jumpTables++;
    }

    public function compared(Aspect $aspect): void
    {
        $this->compared[$aspect->value] = ($this->compared[$aspect->value] ?? 0) + 1;
    }

    public function matchedDecision(): void
    {
        $this->matchedDecisions++;
    }

    public function opcovOnlyDecision(): void
    {
        $this->opcovOnlyDecisions++;
    }

    public function hitCompared(): void
    {
        $this->hitsCompared++;
    }

    public function hitUnknown(): void
    {
        $this->hitsUnknown++;
    }

    public function absorb(self $other): void
    {
        array_push($this->divergences, ...$other->divergences);
        array_push($this->disagreements, ...$other->disagreements);
        $this->anonymousDecisions += $other->anonymousDecisions;
        $this->jumpTables += $other->jumpTables;
        $this->unobservedFiles += $other->unobservedFiles;
        $this->evaluatedEntries += $other->evaluatedEntries;
        $this->expectationsChecked += $other->expectationsChecked;
        $this->unobservedFunctions += $other->unobservedFunctions;
        $this->shiftedTargets += $other->shiftedTargets;
        $this->outcomesNotCompared += $other->outcomesNotCompared;
        foreach ($other->compared as $aspect => $count) {
            $this->compared[$aspect] = ($this->compared[$aspect] ?? 0) + $count;
        }
        $this->matchedDecisions += $other->matchedDecisions;
        $this->opcovOnlyDecisions += $other->opcovOnlyDecisions;
        $this->hitsCompared += $other->hitsCompared;
        $this->hitsUnknown += $other->hitsUnknown;
    }

    /** @return list<Divergence> */
    public function divergences(): array
    {
        return $this->divergences;
    }

    /** @return list<Divergence> */
    public function disagreements(): array
    {
        return $this->disagreements;
    }

    public function comparedCount(Aspect $aspect): int
    {
        return $this->compared[$aspect->value] ?? 0;
    }

    /** @return array<string, int> */
    public function counters(): array
    {
        return [
            'lines compared' => $this->comparedCount(Aspect::Lines),
            'lines where PCOV and Xdebug disagree' => count($this->disagreements),
            'files loaded outside every test (only opcov reports them)' => $this->unobservedFiles,
            "eval()'d code entries (only opcov reports them)" => $this->evaluatedEntries,
            'targets compared with Xdebug in line mode only' => $this->outcomesNotCompared,
            'decisions matched to an Xdebug branch' => $this->matchedDecisions,
            '... of which Xdebug starts a target on an earlier statement line' => $this->shiftedTargets,
            'functions without Xdebug branch data and without opcov hits' => $this->unobservedFunctions,
            'opcov-only decisions (??, ?:, ?->)' => $this->opcovOnlyDecisions,
            'Xdebug switch jump tables (opcov: case chain)' => $this->jumpTables,
            'decisions in anonymous classes (Xdebug cannot name)' => $this->anonymousDecisions,
            'outcome verdicts compared' => $this->hitsCompared,
            'outcome verdicts opcov does not report (Flow edges)' => $this->hitsUnknown,
            'opcov-only outcomes and loop flags checked against fixture expectations' => $this->expectationsChecked,
        ];
    }
}
