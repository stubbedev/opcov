<?php

declare(strict_types=1);

namespace Opcov\Measure\Bench;

use Opcov\Measure\DocSection;
use Opcov\Measure\Driver;
use Opcov\Measure\ReportSize;
use Opcov\Measure\Toolchain;

final readonly class BenchReport
{
    /**
     * @param array<string, string> $versions
     * @param array<string, string> $machine
     * @param array<string, WorkloadResult> $workloads
     */
    public function __construct(
        private Toolchain $toolchain,
        private array $versions,
        private array $machine,
        private int $runs,
        public array $workloads,
    ) {
    }

    /** @return array<string, mixed> */
    public function toArray(): array
    {
        return [
            'php' => $this->toolchain->phpRelease,
            'versions' => $this->versions,
            'machine' => $this->machine,
            'runs' => $this->runs,
            'workloads' => array_map(static fn (WorkloadResult $result): array => $result->toArray(), $this->workloads),
        ];
    }

    public function text(): string
    {
        $text = '';
        foreach ($this->workloads as $name => $result) {
            $text .= "$name ({$result->tests()} tests)\n";
            foreach ([$this->counterRows($result), $this->memoryRows($result)] as $rows) {
                foreach ($rows as $row) {
                    $text .= '  ' . implode('  ', array_map(static fn (string $cell): string => str_pad($cell, 14), $row)) . "\n";
                }
            }
        }
        return $text;
    }

    public function markdown(): string
    {
        $markdown = sprintf(
            "PHP %s, median of %d runs. Machine: %s.\n\nDrivers: %s.\n\nCounters: %s, as `perf stat` reported them.\n",
            $this->toolchain->phpRelease,
            $this->runs,
            implode(', ', array_map(static fn (string $key, string $value): string => "$key $value", array_keys($this->machine), $this->machine)),
            implode(', ', $this->versions),
            $this->eventNames(),
        );
        foreach ($this->workloads as $name => $result) {
            $markdown .= sprintf("\n#### %s\n\n%s; %d tests.\n\n", $name, ucfirst($result->workload->description()), $result->tests());
            $rows = $this->counterRows($result);
            $markdown .= DocSection::table(array_shift($rows), $rows);
            $markdown .= "\n";
            $rows = $this->memoryRows($result);
            $markdown .= DocSection::table(array_shift($rows), $rows);
            foreach ($result->drivers() as $driver) {
                if (!$result->hasTimedOut($driver) && $result->report($driver)->refusal !== null) {
                    $markdown .= sprintf("\n%s refused to write the report: %s\n", $driver->label(), $result->report($driver)->refusal);
                }
            }
        }
        return $markdown;
    }

    private function eventNames(): string
    {
        foreach ($this->workloads as $result) {
            foreach ($result->drivers() as $driver) {
                if (!$result->hasTimedOut($driver)) {
                    return implode(', ', array_map(static fn (string $name): string => "`$name`", $result->reportedEvents($driver)));
                }
            }
        }
        return 'none';
    }

    /** @return list<list<string>> */
    private function counterRows(WorkloadResult $result): array
    {
        $rows = [['driver', 'instructions', 'x none', 'cycles', 'x none', 'branch misses', 'cache misses', 'page faults', 'task-clock s', 'wall s (min-max)', 'x none', 'peak RSS MiB']];
        foreach ($result->drivers() as $driver) {
            if ($result->hasTimedOut($driver)) {
                $rows[] = [$driver->label(), "timed out after {$result->budgetSeconds} s", '', '', '', '', '', '', '', '', '', ''];
                continue;
            }
            $wall = $result->stats($driver, static fn (Sample $sample): float => $sample->seconds);
            $rss = $result->stats($driver, static fn (Sample $sample): int => $sample->peakRssBytes);
            $taskClock = $result->counter($driver, 'task-clock');
            $rows[] = [
                $driver->label(),
                self::count($result->counter($driver, 'instructions')?->median),
                self::ratioCell($result->ratio($driver, static fn (Sample $sample): ?float => $sample->counters->values['instructions'])),
                self::count($result->counter($driver, 'cycles')?->median),
                self::ratioCell($result->ratio($driver, static fn (Sample $sample): ?float => $sample->counters->values['cycles'])),
                self::count($result->counter($driver, 'branch-misses')?->median),
                self::count($result->counter($driver, 'cache-misses')?->median),
                self::count($result->counter($driver, 'page-faults')?->median),
                $taskClock === null ? 'unavailable' : sprintf('%.3f', $taskClock->median / 1000),
                $wall === null ? 'unavailable' : sprintf('%.3f (%.3f-%.3f)', $wall->median, $wall->min, $wall->max),
                self::ratioCell($result->ratio($driver, static fn (Sample $sample): float => $sample->seconds)),
                $rss === null ? 'unavailable' : sprintf('%.1f', $rss->median / 1048576),
            ];
        }
        return $rows;
    }

    /** @return list<list<string>> */
    private function memoryRows(WorkloadResult $result): array
    {
        $rows = [['driver', 'allocation calls (Zend MM)', 'x none', 'bytes allocated (Zend MM)', 'allocation calls (USE_ZEND_ALLOC=0)', 'x none', 'bytes allocated (USE_ZEND_ALLOC=0)', 'PHP peak MiB', 'payload MiB', 'report MiB']];
        $noneSystem = in_array(Driver::None, $result->drivers(), true) && !$result->hasTimedOut(Driver::None) ? $result->systemSample(Driver::None) : null;
        foreach ($result->drivers() as $driver) {
            if ($result->hasTimedOut($driver)) {
                $rows[] = [$driver->label(), "timed out after {$result->budgetSeconds} s", '', '', '', '', '', '', '', ''];
                continue;
            }
            $calls = $result->stats($driver, static fn (Sample $sample): int => $sample->allocations->calls);
            $bytes = $result->stats($driver, static fn (Sample $sample): int => $sample->allocations->bytes);
            $peak = $result->stats($driver, static fn (Sample $sample): int => $sample->phpPeakBytes);
            $system = $result->systemSample($driver);
            $rows[] = [
                $driver->label(),
                self::count($calls?->median),
                self::ratioCell($result->ratio($driver, static fn (Sample $sample): int => $sample->allocations->calls)),
                self::bytes($bytes?->median),
                $system === null ? "timed out after {$result->budgetSeconds} s" : self::count((float) $system->allocations->calls),
                $system === null || $noneSystem === null || $noneSystem->allocations->calls === 0 ? 'n/a' : sprintf('%.2f', $system->allocations->calls / $noneSystem->allocations->calls),
                $system === null ? '' : self::bytes((float) $system->allocations->bytes),
                $peak === null ? 'unavailable' : sprintf('%.1f', $peak->median / 1048576),
                sprintf('%.2f', $result->payloadBytes($driver) / 1048576),
                self::reportCell($result->report($driver)),
            ];
        }
        return $rows;
    }

    private static function count(?float $value): string
    {
        if ($value === null) {
            return 'unavailable';
        }
        foreach ([1_000_000_000 => 'G', 1_000_000 => 'M', 1_000 => 'k'] as $scale => $suffix) {
            if ($value >= $scale) {
                return sprintf('%.3f %s', $value / $scale, $suffix);
            }
        }
        return sprintf('%.0f', $value);
    }

    private static function bytes(?float $value): string
    {
        return $value === null ? 'unavailable' : sprintf('%.1f MiB', $value / 1048576);
    }

    private static function ratioCell(?float $ratio): string
    {
        return $ratio === null ? 'n/a' : sprintf('%.2f', $ratio);
    }

    private static function reportCell(ReportSize $report): string
    {
        if ($report->bytes !== null) {
            return sprintf('%.2f', $report->bytes / 1048576);
        }
        return $report->refusal === null ? 'n/a' : 'refused';
    }
}
