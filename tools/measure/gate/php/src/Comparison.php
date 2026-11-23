<?php

declare(strict_types=1);

namespace Opcov\Measure\Gate;

final readonly class Comparison
{
    private const array SPLIT_KINDS = ['CoalesceValue', 'CoalesceFallback', 'TernaryValue', 'TernaryFallback', 'NullsafeNull', 'NullsafeValue'];

    private const array EXCLUDED_KINDS = ['Engine', 'LoopEnter', 'LoopSkip'];

    /** @param list<Finding> $findings */
    private function __construct(
        public array $findings,
        public int $functions,
        public int $paths,
        public int $pathsNeverTaken,
    ) {
    }

    /**
     * @param array<string, mixed> $opcov
     * @param array<string, mixed> $reference
     * @param array<string, mixed> $report
     */
    public static function of(array $opcov, array $reference, array $report): self
    {
        $opcovFiles = [];
        foreach ($opcov['files'] as $file) {
            foreach ($file['functions'] as $function) {
                $opcovFiles[$file['path']][$function['key']] = $function;
            }
        }
        $findings = [];
        $functions = 0;
        $paths = 0;
        $pathsNeverTaken = 0;
        $files = array_unique([...array_keys($opcovFiles), ...array_keys($reference['files'])]);
        sort($files, SORT_STRING);
        foreach ($files as $file) {
            $xdebugFunctions = $reference['files'][$file] ?? [];
            if (!isset($opcovFiles[$file])) {
                $uncovered = array_sum(array_map(static fn (array $function): int => count($function['uncovered_lines']), $xdebugFunctions));
                $findings[] = new Finding(Category::FileOnlyXdebug, $file, '', 0, sprintf('%d function(s), %d uncovered line(s)', count($xdebugFunctions), $uncovered));
                continue;
            }
            $keys = array_unique([...array_keys($opcovFiles[$file]), ...array_keys($xdebugFunctions)]);
            sort($keys, SORT_STRING);
            foreach ($keys as $key) {
                $o = $opcovFiles[$file][$key] ?? null;
                $x = $xdebugFunctions[$key] ?? null;
                if ($x === null) {
                    $findings[] = new Finding(Category::FunctionOnlyOpcov, $file, $key, $o['span'][0], sprintf('lines %d-%d', $o['span'][0], $o['span'][2]));
                    continue;
                }
                if ($o === null) {
                    $findings[] = new Finding(Category::FunctionOnlyXdebug, $file, $key, $x['span'][0], sprintf('lines %d-%d', $x['span'][0], $x['span'][1]));
                    continue;
                }
                $functions++;
                $paths += $x['paths'];
                $pathsNeverTaken += $x['paths_never_taken'];
                array_push($findings, ...self::function($file, $key, $o, $x, $report['files'][$file]['functions'][$key]['outcomes'] ?? []));
            }
        }
        return new self($findings, $functions, $paths, $pathsNeverTaken);
    }

    /**
     * @param array<string, mixed> $o
     * @param array<string, mixed> $x
     * @param list<array<string, mixed>> $reportOutcomes
     * @return list<Finding>
     */
    private static function function(string $file, string $key, array $o, array $x, array $reportOutcomes): array
    {
        $finding = static fn (Category $category, int $line, string $detail = ''): Finding => new Finding($category, $file, $key, $line, $detail);
        $opcovUncovered = array_fill_keys($o['uncovered_lines'], true);
        $opcovExecutable = array_fill_keys(array_column($o['lines'], 'line'), true);
        $xdebugUncovered = array_fill_keys($x['uncovered_lines'], true);
        $xdebugExecutable = array_fill_keys($x['executable_lines'], true);
        $findings = [];
        foreach (array_keys($xdebugUncovered) as $line) {
            $findings[] = match (true) {
                isset($opcovUncovered[$line]) => $finding(Category::LineBoth, $line),
                isset($opcovExecutable[$line]) => $finding(Category::LineXdebugOnlyCoveredByOpcov, $line),
                default => $finding(Category::LineXdebugOnlyNotExecutableForOpcov, $line),
            };
        }
        foreach (array_keys($opcovUncovered) as $line) {
            if (isset($xdebugUncovered[$line])) {
                continue;
            }
            $findings[] = $finding(isset($xdebugExecutable[$line]) ? Category::LineOpcovOnlyCoveredByXdebug : Category::LineOpcovOnlyNotExecutableForXdebug, $line);
        }
        $fallback = self::framelessFallbackOutcomes($reportOutcomes, $opcovUncovered);
        $unseenAt = [];
        foreach ($o['unseen_outcomes'] as $outcome) {
            if (!isset($fallback[$outcome['id']])) {
                $unseenAt[$outcome['line']][] = self::kind($outcome['kind']);
            }
        }
        $excludedAt = [];
        foreach ($reportOutcomes as $outcome) {
            $kind = self::kind($outcome['kind']);
            if ($outcome['hits'] === [] && in_array($kind, self::EXCLUDED_KINDS, true)) {
                $excludedAt[$outcome['line']][] = $kind;
            }
        }
        $unexecutedEntries = [];
        foreach ($x['unexecuted_branches'] as $branch) {
            $low = min($branch['line_start'], $branch['line_end']);
            $high = max($branch['line_start'], $branch['line_end']);
            $ranDecisions = array_values(array_unique(array_column(array_filter($branch['entries'], static fn (array $entry): bool => $entry['ran'] && $entry['decision']), 'line')));
            foreach ($ranDecisions as $line) {
                $unexecutedEntries[$line] = true;
            }
            $detail = sprintf('block at opcode %d, lines %d-%d, entered from line(s) %s', $branch['op_start'], $low, $high, $branch['entries'] === [] ? 'none' : implode(', ', array_unique(array_column($branch['entries'], 'line'))));
            $excluded = array_merge(...array_map(static fn (int $line): array => $excludedAt[$line] ?? [], $ranDecisions));
            $findings[] = match (true) {
                array_any(range($low, $high), static fn (int $line): bool => isset($opcovUncovered[$line])) => $finding(Category::BranchOnUncoveredLine, $low, $detail),
                array_any($ranDecisions, static fn (int $line): bool => isset($unseenAt[$line])) => $finding(Category::BranchUnseenOutcome, $low, $detail),
                $branch['entries'] !== [] && !array_any($branch['entries'], static fn (array $entry): bool => $entry['ran']) => $finding(Category::BranchBehindUnexecutedBranch, $low, $detail),
                $excluded !== [] => $finding(Category::BranchExcludedOutcome, $low, $detail . '; opcov: unseen ' . implode(', ', array_unique($excluded))),
                default => $finding(Category::BranchUnmatched, $low, $detail),
            };
        }
        $edgeOnly = [];
        foreach ($x['decisions'] as $decision) {
            if ($decision['all_successors_ran']) {
                $edgeOnly[$decision['line']] = true;
            }
        }
        foreach ($o['unseen_outcomes'] as $outcome) {
            $kind = self::kind($outcome['kind']);
            $line = $outcome['line'];
            $findings[] = match (true) {
                isset($opcovUncovered[$line]) => $finding(Category::OutcomeOnUncoveredLine, $line, $kind),
                isset($fallback[$outcome['id']]) => $finding(Category::OutcomeInFramelessFallback, $line, $kind),
                in_array($kind, self::SPLIT_KINDS, true) => $finding(Category::OutcomeSplit, $line, $kind),
                isset($unexecutedEntries[$line]) => $finding(Category::OutcomeUnexecutedBranch, $line, $kind),
                isset($edgeOnly[$line]) => $finding(Category::OutcomeEdge, $line, $kind),
                default => $finding(Category::OutcomeUnclassified, $line, $kind),
            };
        }
        foreach ($o['loops_never_zero'] as $loop) {
            $findings[] = $finding(Category::LoopZero, $loop['line'], $loop['kind']);
        }
        foreach ($o['loops_never_twice'] as $loop) {
            $findings[] = $finding(Category::LoopSecond, $loop['line'], $loop['kind']);
        }
        return $findings;
    }

    /**
     * @param list<array<string, mixed>> $outcomes
     * @param array<int, true> $uncovered
     * @return array<string, true>
     */
    private static function framelessFallbackOutcomes(array $outcomes, array $uncovered): array
    {
        $fallbackFrom = [];
        $sites = [];
        foreach ($outcomes as $outcome) {
            if (self::kind($outcome['kind']) === 'Engine' && $outcome['hits'] === []) {
                $fallbackFrom[$outcome['line']] = min($fallbackFrom[$outcome['line']] ?? PHP_INT_MAX, $outcome['site']);
            }
            $sites[$outcome['site']][] = $outcome;
        }
        $ids = [];
        foreach ($sites as $site => $construct) {
            $line = $construct[0]['line'];
            $kind = self::kind($construct[0]['kind']);
            if (isset($uncovered[$line]) || in_array($kind, self::EXCLUDED_KINDS, true) || !isset($fallbackFrom[$line]) || $site < $fallbackFrom[$line]) {
                continue;
            }
            if (array_any($construct, static fn (array $outcome): bool => $outcome['hits'] !== [])) {
                continue;
            }
            foreach ($construct as $outcome) {
                $ids[$outcome['id']] = true;
            }
        }
        return $ids;
    }

    private static function kind(mixed $kind): string
    {
        if (is_string($kind)) {
            return $kind;
        }
        if (is_array($kind) && count($kind) === 1) {
            return sprintf('%s(%s)', array_key_first($kind), json_encode(reset($kind), JSON_THROW_ON_ERROR));
        }
        throw new \UnexpectedValueException('unknown outcome kind shape ' . json_encode($kind));
    }

    /** @return array<string, int> */
    public function counts(): array
    {
        $counts = array_fill_keys(array_map(static fn (Category $category): string => $category->value, Category::cases()), 0);
        foreach ($this->findings as $finding) {
            $counts[$finding->category->value]++;
        }
        return $counts;
    }
}
