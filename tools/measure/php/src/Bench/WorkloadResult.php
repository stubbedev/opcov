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

    /** @var array<string, list<Sample>> */
    private array $samples = [];

    /** @var array<string, Sample> */
    private array $systemAllocator = [];

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
        unset($this->samples[$driver->value], $this->systemAllocator[$driver->value], $this->payloadBytes[$driver->value], $this->reports[$driver->value]);
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

    public function timed(Driver $driver, int $tests, Sample $sample): void
    {
        $this->expectTests($driver, $tests);
        $this->samples[$driver->value][] = $sample;
    }

    public function systemAllocator(Driver $driver, int $tests, Sample $sample): void
    {
        $this->expectTests($driver, $tests);
        $this->systemAllocator[$driver->value] = $sample;
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
            fn (Driver $driver): bool => isset($this->samples[$driver->value]) || isset($this->timedOut[$driver->value]),
        ));
    }

    /** @param \Closure(Sample): (float|int|null) $metric */
    public function stats(Driver $driver, \Closure $metric): ?Stats
    {
        $values = [];
        foreach ($this->samples[$driver->value] ?? throw new \LogicException("{$this->workload->value} has no samples for {$driver->value}") as $sample) {
            $value = $metric($sample);
            if ($value === null) {
                return null;
            }
            $values[] = $value;
        }
        return Stats::of($values);
    }

    public function counter(Driver $driver, string $event): ?Stats
    {
        return $this->stats($driver, static fn (Sample $sample): ?float => $sample->counters->values[$event] ?? throw new \LogicException("no $event counter"));
    }

    /** @return array<string, string> */
    public function reportedEvents(Driver $driver): array
    {
        return ($this->samples[$driver->value] ?? throw new \LogicException("{$this->workload->value} has no samples for {$driver->value}"))[0]->counters->reported;
    }

    /** @param \Closure(Sample): (float|int|null) $metric */
    public function ratio(Driver $driver, \Closure $metric): ?float
    {
        if (!isset($this->samples[Driver::None->value])) {
            return null;
        }
        $mine = $this->stats($driver, $metric);
        $none = $this->stats(Driver::None, $metric);
        if ($mine === null || $none === null || $none->median <= 0.0) {
            return null;
        }
        return $mine->median / $none->median;
    }

    public function systemSample(Driver $driver): ?Sample
    {
        return $this->systemAllocator[$driver->value] ?? null;
    }

    public function payloadBytes(Driver $driver): int
    {
        return $this->payloadBytes[$driver->value] ?? throw new \LogicException("{$this->workload->value} has no payload size for {$driver->value}");
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
                'runs' => array_map(static fn (Sample $sample): array => $sample->toArray(), $this->samples[$driver->value]),
                'system_allocator_run' => $this->systemSample($driver)?->toArray() ?? ['timed_out_after_seconds' => $this->budgetSeconds],
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
