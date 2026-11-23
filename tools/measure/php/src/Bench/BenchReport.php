<?php

declare(strict_types=1);

namespace Opcov\Measure\Bench;

use Opcov\Measure\DocSection;
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
            foreach ($this->rows($result) as $row) {
                $text .= sprintf("  %-34s %10s %10s %10s %8s %12s %12s %10s\n", ...$row);
            }
        }
        return $text;
    }

    public function markdown(): string
    {
        $markdown = sprintf(
            "PHP %s, median of %d runs. Machine: %s.\n\nDrivers: %s.\n",
            $this->toolchain->phpRelease,
            $this->runs,
            implode(', ', array_map(static fn (string $key, string $value): string => "$key $value", array_keys($this->machine), $this->machine)),
            implode(', ', $this->versions),
        );
        foreach ($this->workloads as $name => $result) {
            $markdown .= sprintf("\n#### %s\n\n%s; %d tests.\n\n", $name, ucfirst($result->workload->description()), $result->tests());
            $rows = $this->rows($result);
            $markdown .= DocSection::table(array_shift($rows), $rows);
        }
        return $markdown;
    }

    /** @return list<list<string>> */
    private function rows(WorkloadResult $result): array
    {
        $rows = [['driver', 'median s', 'min s', 'max s', 'x none', 'peak RSS MiB', 'payload MiB', 'report MiB']];
        foreach ($result->drivers() as $driver) {
            if ($result->hasTimedOut($driver)) {
                $rows[] = [$driver->label(), "> {$result->budgetSeconds}", 'timed out', '', '', '', '', ''];
                continue;
            }
            $time = $result->time($driver);
            $overhead = $result->overhead($driver);
            $rows[] = [
                $driver->label(),
                sprintf('%.3f', $time->median),
                sprintf('%.3f', $time->min),
                sprintf('%.3f', $time->max),
                $overhead === null ? 'n/a' : sprintf('%.2f', $overhead),
                sprintf('%.1f', $result->rss($driver)->median / 1048576),
                sprintf('%.2f', $result->payloadBytes($driver) / 1048576),
                $result->reportBytes($driver) === null ? 'n/a' : sprintf('%.2f', $result->reportBytes($driver) / 1048576),
            ];
        }
        return $rows;
    }
}
