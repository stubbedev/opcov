<?php

declare(strict_types=1);

namespace Opcov\Measure\Bench;

use Opcov\Measure\Driver;
use Opcov\Measure\ReportSize;

final class WorkloadResult
{
    private ?int $tests = null;

    /** @var array<string, int> */
    private array $payloadBytes = [];

    /** @var array<string, ReportSize> */
    private array $reports = [];

    /** @var array<string, list<float>> */
    private array $seconds = [];

    /** @var array<string, list<int>> */
    private array $rss = [];

    /** @var array<string, true> */
    private array $timedOut = [];

    public function __construct(
        public readonly BenchWorkload $workload,
        public readonly int $budgetSeconds,
    ) {
    }

    public function timedOut(Driver $driver): void
    {
        $this->timedOut[$driver->value] = true;
        unset($this->seconds[$driver->value], $this->rss[$driver->value], $this->payloadBytes[$driver->value], $this->reports[$driver->value]);
    }

    public function hasTimedOut(Driver $driver): bool
    {
        return isset($this->timedOut[$driver->value]);
    }

    public function payload(Driver $driver, int $tests, int $bytes, ReportSize $report): void
    {
        $this->expectTests($driver, $tests);
        $this->payloadBytes[$driver->value] = $bytes;
        $this->reports[$driver->value] = $report;
    }

    public function report(Driver $driver): ReportSize
    {
        return $this->reports[$driver->value] ?? throw new \LogicException("{$this->workload->value} has no report size for {$driver->value}");
    }

    public function timed(Driver $driver, int $tests, float $seconds, int $rssBytes): void
    {
        $this->expectTests($driver, $tests);
        $this->seconds[$driver->value][] = $seconds;
        $this->rss[$driver->value][] = $rssBytes;
    }

    public function tests(): int
    {
        return $this->tests ?? throw new \LogicException("{$this->workload->value} has no measurements");
    }

    /** @return list<Driver> */
    public function drivers(): array
    {
        return array_values(array_filter(
            Driver::cases(),
            fn (Driver $driver): bool => isset($this->seconds[$driver->value]) || isset($this->timedOut[$driver->value]),
        ));
    }

    public function time(Driver $driver): Stats
    {
        return Stats::of($this->seconds[$driver->value] ?? throw new \LogicException("{$this->workload->value} has no times for {$driver->value}"));
    }

    public function rss(Driver $driver): Stats
    {
        return Stats::of($this->rss[$driver->value] ?? throw new \LogicException("{$this->workload->value} has no RSS for {$driver->value}"));
    }

    public function payloadBytes(Driver $driver): int
    {
        return $this->payloadBytes[$driver->value] ?? throw new \LogicException("{$this->workload->value} has no payload size for {$driver->value}");
    }

    public function overhead(Driver $driver): ?float
    {
        if (!isset($this->seconds[Driver::None->value])) {
            return null;
        }
        return $this->time($driver)->median / $this->time(Driver::None)->median;
    }

    /** @return array<string, mixed> */
    public function toArray(): array
    {
        $drivers = [];
        foreach ($this->drivers() as $driver) {
            if ($this->hasTimedOut($driver)) {
                $drivers[$driver->value] = ['timed_out_after_seconds' => $this->budgetSeconds];
                continue;
            }
            $drivers[$driver->value] = [
                'seconds' => $this->seconds[$driver->value],
                'peak_rss_bytes' => $this->rss[$driver->value],
                'payload_bytes' => $this->payloadBytes($driver),
                'report' => $this->report($driver)->toArray(),
            ];
        }
        return ['tests' => $this->tests(), 'drivers' => $drivers];
    }

    private function expectTests(Driver $driver, int $tests): void
    {
        if ($this->tests !== null && $this->tests !== $tests) {
            throw new \RuntimeException(sprintf('%s ran %d tests under %s, %d before', $this->workload->value, $tests, $driver->value, $this->tests));
        }
        $this->tests = $tests;
    }
}
