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

    /** @var array<string, TimedOut> */
    private array $timedOut = [];

    /** @var array<string, TimedOut> */
    private array $systemTimedOut = [];

    private function __construct(public readonly BenchWorkload $workload)
    {
    }

    /**
     * @param list<Driver> $drivers
     * @param \Closure(Key): Record $lookup
     */
    public static function collect(BenchWorkload $workload, array $drivers, int $runs, \Closure $lookup): self
    {
        $result = new self($workload);
        foreach ($drivers as $driver) {
            $result->collectDriver($driver, $runs, $lookup);
        }
        return $result;
    }

    public function timeout(Driver $driver): ?TimedOut
    {
        return $this->timedOut[$driver->value] ?? null;
    }

    public function hasTimedOut(Driver $driver): bool
    {
        return isset($this->timedOut[$driver->value]);
    }

    public function report(Driver $driver): ReportSize
    {
        return $this->reports[$driver->value] ?? throw new \LogicException("{$this->workload->value} has no report size for {$driver->value}");
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

    public function systemTimeout(Driver $driver): ?TimedOut
    {
        return $this->systemTimedOut[$driver->value] ?? null;
    }

    public function payloadBytes(Driver $driver): int
    {
        return $this->payloadBytes[$driver->value] ?? throw new \LogicException("{$this->workload->value} has no payload size for {$driver->value}");
    }

    /** @param \Closure(Key): Record $lookup */
    private function collectDriver(Driver $driver, int $runs, \Closure $lookup): void
    {
        $payload = $lookup(Key::payload($this->workload, $driver))->result;
        if ($payload instanceof TimedOut) {
            $this->timedOut[$driver->value] = $payload;
            return;
        }
        if (!$payload instanceof PayloadSize) {
            throw new \LogicException("{$this->workload->value} {$driver->value}: a payload record holds " . $payload::class);
        }
        $this->expectTests($driver, $payload->tests);
        $this->payloadBytes[$driver->value] = $payload->bytes;
        $this->reports[$driver->value] = $payload->report;
        $samples = [];
        for ($round = 1; $round <= $runs; $round++) {
            $run = $lookup(Key::run($this->workload, $driver, $round))->result;
            if ($run instanceof TimedOut) {
                $this->timedOut[$driver->value] = $run;
                unset($this->payloadBytes[$driver->value], $this->reports[$driver->value]);
                return;
            }
            $samples[] = $this->measured($driver, $run);
        }
        $this->samples[$driver->value] = $samples;
        $system = $lookup(Key::systemAllocator($this->workload, $driver))->result;
        if ($system instanceof TimedOut) {
            $this->systemTimedOut[$driver->value] = $system;
            return;
        }
        $this->systemAllocator[$driver->value] = $this->measured($driver, $system);
    }

    private function measured(Driver $driver, Result $result): Sample
    {
        if (!$result instanceof Measured) {
            throw new \LogicException("{$this->workload->value} {$driver->value}: a timed record holds " . $result::class);
        }
        $this->expectTests($driver, $result->tests);
        return $result->sample;
    }

    private function expectTests(Driver $driver, int $tests): void
    {
        if ($this->tests !== null && $this->tests !== $tests) {
            throw new \RuntimeException(sprintf('%s ran %d tests under %s, %d before', $this->workload->value, $tests, $driver->value, $this->tests));
        }
        $this->tests = $tests;
    }
}
