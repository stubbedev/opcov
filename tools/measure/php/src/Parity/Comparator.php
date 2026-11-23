<?php

declare(strict_types=1);

namespace Opcov\Measure\Parity;

use Opcov\Measure\Coverage;
use Opcov\Measure\Decision;
use Opcov\Measure\Driver;
use Opcov\Measure\Outcome;

final class Comparator
{

    private const string ANONYMOUS_CLASS = 'class@anonymous';
    /** @param \Closure(string): int $lastLine */
    public static function lines(string $label, Coverage $opcov, Coverage $pcov, Driver $xdebugDriver, Coverage $xdebug, \Closure $lastLine): Findings
    {
        $findings = new Findings();
        $files = array_values(array_unique([...$opcov->files(), ...$pcov->files(), ...$xdebug->files()]));
        sort($files, SORT_STRING);
        foreach ($files as $file) {
            $opcovLines = $opcov->lines($file);
            $pcovLines = $pcov->lines($file);
            $xdebugLines = $xdebug->lines($file);
            if ($pcovLines === [] && $xdebugLines === [] && !in_array(true, $opcovLines, true)) {
                $findings->unobserved();
                continue;
            }
            $lines = array_keys($opcovLines + $pcovLines + $xdebugLines);
            sort($lines);
            foreach ($lines as $line) {
                $findings->compared(Aspect::Lines);
                $states = [
                    Driver::Opcov->value => self::lineState($opcovLines[$line] ?? null),
                    Driver::Pcov->value => self::lineState($pcovLines[$line] ?? null),
                    $xdebugDriver->value => self::lineState($xdebugLines[$line] ?? null),
                ];
                if (count(array_unique($states)) === 1) {
                    continue;
                }
                $detail = implode(', ', array_map(static fn (string $driver, string $state): string => "$driver: $state", array_keys($states), $states));
                $opcovState = $states[Driver::Opcov->value];
                if ($opcovState === $states[Driver::Pcov->value] || $opcovState === $states[$xdebugDriver->value]) {
                    $findings->disagree(new Divergence(Aspect::Lines, "$label/$file", $line, new Pair(Driver::Pcov, $xdebugDriver), $detail));
                    continue;
                }
                $declarationsOnly = $opcovState === self::lineState(null)
                    && $states[Driver::Pcov->value] !== $opcovState
                    && $states[$xdebugDriver->value] !== $opcovState
                    && $line >= $lastLine($file);
                $findings->diverge(new Divergence(Aspect::Lines, "$label/$file", $line, new Pair(Driver::Opcov, Driver::Pcov), $detail, $declarationsOnly ? Rule::DeclarationsOnlyReturn : null));
            }
        }
        return $findings;
    }

    public static function outcomes(string $label, Coverage $opcov, Coverage $xdebug): Findings
    {
        $findings = new Findings();
        $files = array_values(array_unique([...$opcov->files(), ...$xdebug->files()]));
        sort($files, SORT_STRING);
        foreach ($files as $file) {
            $opcovFunctions = $opcov->decisions($file);
            $xdebugFunctions = $xdebug->decisions($file);
            $functions = array_keys($opcovFunctions + $xdebugFunctions);
            sort($functions, SORT_STRING);
            foreach ($functions as $function) {
                if (str_contains($function, self::ANONYMOUS_CLASS)) {
                    $findings->anonymous(count($opcovFunctions[$function] ?? []));
                    continue;
                }
                if (!isset($xdebugFunctions[$function]) && !self::anyHit($opcovFunctions[$function] ?? [])) {
                    $findings->unobservedFunction();
                    continue;
                }
                self::alignFunction(
                    $findings,
                    "$label/$file",
                    $function,
                    new Pair(Driver::Opcov, Driver::XdebugBranch),
                    $opcovFunctions[$function] ?? [],
                    $xdebugFunctions[$function] ?? [],
                );
            }
        }
        return $findings;
    }

    /**
     * @param list<Decision> $opcov
     * @param list<Decision> $xdebug
     */
    private static function alignFunction(Findings $findings, string $file, string $function, Pair $pair, array $opcov, array $xdebug): void
    {
        $opcovLines = array_fill_keys(array_map(static fn (Decision $decision): int => $decision->line, $opcov), true);
        $comparable = [];
        foreach ($opcov as $decision) {
            if (self::opcovOnly($decision)) {
                $findings->opcovOnlyDecision();
                continue;
            }
            $comparable[] = $decision;
        }
        foreach (self::align($comparable, $xdebug) as [$left, $right]) {
            if ($left !== null && $right !== null) {
                $findings->compared(Aspect::Outcomes);
                $findings->matchedDecision();
                if ($left->targets() !== $right->targets()) {
                    $findings->shiftedTargets();
                }
                foreach (array_map(null, $left->outcomes, $right->outcomes) as [$leftOutcome, $rightOutcome]) {
                    self::compareHits($findings, $file, $function, $pair, $left, $leftOutcome, $rightOutcome);
                }
                continue;
            }
            if ($left !== null) {
                $findings->compared(Aspect::Outcomes);
                $findings->diverge(new Divergence(
                    Aspect::Outcomes,
                    $file,
                    $left->line,
                    $pair,
                    sprintf('%s: opcov decides %s, Xdebug has no matching branch', $function, self::describe($left)),
                ));
                continue;
            }
            if ($right !== null) {
                if (count($right->outcomes) > 2 && isset($opcovLines[$right->outcomes[0]->target])) {
                    $findings->jumpTable();
                    continue;
                }
                $findings->compared(Aspect::Outcomes);
                $findings->diverge(new Divergence(
                    Aspect::Outcomes,
                    $file,
                    $right->line,
                    $pair,
                    sprintf('%s: Xdebug branches %s, opcov has no matching decision', $function, self::describe($right)),
                ));
            }
        }
    }

    private static function compareHits(Findings $findings, string $file, string $function, Pair $pair, Decision $decision, Outcome $opcov, Outcome $xdebug): void
    {
        if ($opcov->hit === null || $xdebug->hit === null) {
            $findings->hitUnknown();
            return;
        }
        $findings->hitCompared();
        if ($opcov->hit === $xdebug->hit) {
            return;
        }
        $findings->diverge(new Divergence(
            Aspect::Outcomes,
            $file,
            $decision->line,
            $pair,
            sprintf('%s: outcome %s to line %d: opcov %s, Xdebug %s', $function, $opcov->kind ?? '?', $opcov->target, self::hitWord($opcov->hit), self::hitWord($xdebug->hit)),
        ));
    }

    /**
     * @param list<Decision> $left
     * @param list<Decision> $right
     * @return list<array{0: ?Decision, 1: ?Decision}>
     */
    private static function align(array $left, array $right): array
    {
        $rows = count($left);
        $columns = count($right);
        $length = array_fill(0, $rows + 1, array_fill(0, $columns + 1, 0));
        for ($i = $rows - 1; $i >= 0; $i--) {
            for ($j = $columns - 1; $j >= 0; $j--) {
                $length[$i][$j] = self::matches($left[$i], $right[$j])
                    ? $length[$i + 1][$j + 1] + 1
                    : max($length[$i + 1][$j], $length[$i][$j + 1]);
            }
        }
        $pairs = [];
        $i = 0;
        $j = 0;
        while ($i < $rows && $j < $columns) {
            if (self::matches($left[$i], $right[$j])) {
                $pairs[] = [$left[$i++], $right[$j++]];
            } elseif ($length[$i + 1][$j] >= $length[$i][$j + 1]) {
                $pairs[] = [$left[$i++], null];
            } else {
                $pairs[] = [null, $right[$j++]];
            }
        }
        for (; $i < $rows; $i++) {
            $pairs[] = [$left[$i], null];
        }
        for (; $j < $columns; $j++) {
            $pairs[] = [null, $right[$j]];
        }
        return $pairs;
    }

    private static function matches(Decision $opcov, Decision $xdebug): bool
    {
        return $opcov->line === $xdebug->line && count($opcov->outcomes) === count($xdebug->outcomes);
    }

    /** @param list<Decision> $decisions */
    private static function anyHit(array $decisions): bool
    {
        foreach ($decisions as $decision) {
            foreach ($decision->outcomes as $outcome) {
                if ($outcome->hit === true) {
                    return true;
                }
            }
        }
        return false;
    }

    private static function opcovOnly(Decision $decision): bool
    {
        foreach ($decision->outcomes as $outcome) {
            if (!in_array($outcome->kind, Expectations::OPCOV_ONLY_KINDS, true)) {
                return false;
            }
        }
        return true;
    }

    private static function describe(Decision $decision): string
    {
        return sprintf('on line %d to lines [%s]', $decision->line, implode(', ', array_map(
            static fn (Outcome $outcome): string => $outcome->kind === null ? (string) $outcome->target : "$outcome->target $outcome->kind",
            $decision->outcomes,
        )));
    }

    private static function lineState(?bool $hit): string
    {
        return match ($hit) {
            null => 'not executable',
            false => 'not covered',
            true => 'covered',
        };
    }

    private static function hitWord(bool $hit): string
    {
        return $hit ? 'taken' : 'not taken';
    }
}
