<?php

declare(strict_types=1);

namespace Opcov\Measure\Parity;

use Opcov\Measure\DocSection;
use Opcov\Measure\Toolchain;

final readonly class ParityReport
{
    /** @param array<string, Findings> $suites */
    public function __construct(
        private Toolchain $toolchain,
        private array $suites,
        private Verdict $verdict,
    ) {
    }

    public function text(): string
    {
        $lines = ["parity on PHP {$this->toolchain->phpRelease} (" . implode(', ', $this->versions()) . ')'];
        foreach ($this->suites as $suite => $findings) {
            $lines[] = "  $suite:";
            foreach ($findings->counters() as $name => $count) {
                $lines[] = sprintf('    %-44s %d', $name, $count);
            }
            $lines[] = sprintf('    %-44s %d', 'divergences', count($findings->divergences()));
        }
        $lines[] = sprintf('allowlisted divergences: %d', count($this->verdict->explained));
        $lines[] = sprintf('unexplained divergences: %d', count($this->verdict->unexplained));
        foreach ($this->verdict->unexplained as $divergence) {
            $lines[] = '  ' . $divergence->describe();
        }
        $lines[] = sprintf('stale allowlist entries: %d', count($this->verdict->stale));
        foreach ($this->verdict->stale as $entry) {
            $lines[] = '  ' . $entry->describe();
        }
        $lines[] = $this->verdict->passed() ? 'parity: passed' : 'parity: FAILED';
        return implode("\n", $lines) . "\n";
    }

    public function markdown(): string
    {
        $rows = [];
        foreach (array_keys((new Findings())->counters()) as $name) {
            $rows[] = [$name, ...array_map(static fn (Findings $findings): string => (string) $findings->counters()[$name], array_values($this->suites))];
        }
        $rows[] = ['divergences', ...array_map(static fn (Findings $findings): string => (string) count($findings->divergences()), array_values($this->suites))];
        $markdown = sprintf("PHP %s; %s.\n\n", $this->toolchain->phpRelease, implode(', ', $this->versions()));
        $markdown .= DocSection::table(['', ...array_keys($this->suites)], $rows);
        $markdown .= sprintf(
            "\nVerdict: %s. %d allowlisted, %d unexplained divergences, %d stale allowlist entries.\n",
            $this->verdict->passed() ? 'passed' : 'failed',
            count($this->verdict->explained),
            count($this->verdict->unexplained),
            count($this->verdict->stale),
        );
        $byRule = [];
        $entries = [];
        foreach ($this->verdict->explained as [$divergence, $entry]) {
            if ($entry instanceof RuleEntry) {
                $byRule[$entry->rule->value] = [$entry, ($byRule[$entry->rule->value][1] ?? 0) + 1];
                continue;
            }
            $entries[] = [
                "{$divergence->file}:{$divergence->line}",
                $divergence->aspect->value,
                $divergence->pair->name(),
                $divergence->detail,
                sprintf('%s ([#%s](%s))', $entry->reason, basename($entry->issue), $entry->issue),
            ];
        }
        if ($byRule !== []) {
            $markdown .= "\n" . DocSection::table(
                ['rule', 'divergences', 'reason'],
                array_values(array_map(static fn (array $rule): array => [
                    "`{$rule[0]->rule->value}`",
                    (string) $rule[1],
                    sprintf('%s ([#%s](%s))', $rule[0]->reason, basename($rule[0]->issue), $rule[0]->issue),
                ], $byRule)),
            );
        }
        if ($entries !== []) {
            $markdown .= "\n" . DocSection::table(['file:line', 'aspect', 'drivers', 'difference', 'reason'], $entries);
        }
        if ($this->verdict->unexplained !== []) {
            $markdown .= "\nUnexplained:\n\n" . implode("\n", array_map(static fn (Divergence $divergence): string => '- `' . $divergence->describe() . '`', $this->verdict->unexplained)) . "\n";
        }
        return $markdown;
    }

    /** @return list<string> */
    private function versions(): array
    {
        return array_values(array_unique(array_map(fn ($driver): string => $driver->version($this->toolchain), ParityRun::DRIVERS)));
    }
}
