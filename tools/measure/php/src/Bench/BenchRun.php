<?php

declare(strict_types=1);

namespace Opcov\Measure\Bench;

use Opcov\Measure\Arguments;
use Opcov\Measure\BudgetExceeded;
use Opcov\Measure\Child;
use Opcov\Measure\Cli;
use Opcov\Measure\DocSection;
use Opcov\Measure\Driver;
use Opcov\Measure\Json;
use Opcov\Measure\Mode;
use Opcov\Measure\Target;
use Opcov\Measure\Toolchain;

final readonly class BenchRun
{
    private const float REGRESSION_TOLERANCE = 1.10;

    /**
     * @param list<BenchWorkload> $workloads
     * @param list<Driver> $drivers
     */
    private function __construct(
        private Toolchain $toolchain,
        private array $workloads,
        private array $drivers,
        private int $runs,
        private int $budgetSeconds,
        private bool $writeDocs,
        private bool $check,
        private bool $updateBaseline,
    ) {
    }

    public static function fromArguments(Arguments $arguments, Toolchain $toolchain): self
    {
        $workloads = array_map(BenchWorkload::parse(...), $arguments->all('--workload', array_map(static fn (BenchWorkload $workload): string => $workload->value, BenchWorkload::cases())));
        $drivers = array_map(Driver::parse(...), $arguments->all('--driver', array_map(
            static fn (Driver $driver): string => $driver->value,
            array_values(array_filter(Driver::cases(), static fn (Driver $driver): bool => $driver->available($toolchain))),
        )));
        foreach ($drivers as $driver) {
            if (!$driver->available($toolchain)) {
                throw new \InvalidArgumentException(sprintf('%s does not build for PHP %s in this environment', $driver->label(), $toolchain->phpVersion()));
            }
        }
        if (($arguments->flag('--check') || $arguments->flag('--update-baseline')) && (!in_array(Driver::None, $drivers, true) || !in_array(Driver::Opcov, $drivers, true))) {
            throw new \InvalidArgumentException('--check and --update-baseline compare opcov against no coverage; both drivers must run');
        }
        return new self(
            $toolchain,
            $workloads,
            $drivers,
            $arguments->positiveInt('--runs', 3),
            $arguments->positiveInt('--budget', 900),
            $arguments->flag('--write-docs'),
            $arguments->flag('--check'),
            $arguments->flag('--update-baseline'),
        );
    }

    public function execute(): int
    {
        $machine = Machine::describe();
        $results = [];
        foreach ($this->workloads as $workload) {
            $results[$workload->value] = $this->measure($workload);
        }
        $report = new BenchReport($this->toolchain, $this->versions(), $machine, $this->runs, $results);
        Json::write(sprintf('%s/bench/%s/results.json', $this->toolchain->cache, $this->toolchain->phpVersion()), $report->toArray());
        echo $report->text();
        if ($this->writeDocs) {
            DocSection::replace("{$this->toolchain->root}/docs/benchmarks.md", 'bench:php' . str_replace('.', '', $this->toolchain->phpVersion()), $report->markdown());
        }
        $baseline = new Baseline("{$this->toolchain->root}/bench/baseline.json");
        if ($this->updateBaseline) {
            $baseline->store($this->toolchain->phpVersion(), $report);
        }
        if ($this->check) {
            foreach ($baseline->warnings($this->toolchain->phpVersion(), $report, self::REGRESSION_TOLERANCE) as $warning) {
                fwrite(STDERR, "bench: warning (cycles depend on machine load): $warning\n");
            }
            $regressions = $baseline->regressions($this->toolchain->phpVersion(), $report, self::REGRESSION_TOLERANCE);
            foreach ($regressions as $regression) {
                fwrite(STDERR, "bench: regression: $regression\n");
            }
            return $regressions === [] ? Cli::PASSED : Cli::FAILED;
        }
        return Cli::PASSED;
    }

    private function measure(BenchWorkload $workload): WorkloadResult
    {
        $target = $workload->target($this->toolchain);
        $directory = sprintf('%s/bench/%s/%s', $this->toolchain->cache, $this->toolchain->phpVersion(), $workload->value);
        $result = new WorkloadResult($workload, $this->budgetSeconds);
        foreach ($this->drivers as $driver) {
            fwrite(STDERR, "bench: {$workload->value} {$driver->value} payload\n");
            try {
                $payload = (new Child($this->toolchain, $target, $driver, Mode::Payload))->run("$directory/{$driver->value}-payload.json", $this->budgetSeconds);
            } catch (BudgetExceeded $e) {
                fwrite(STDERR, "bench: {$e->getMessage()}\n");
                $result->timedOut($driver);
                continue;
            }
            $result->payload($driver, $payload->tests(), $payload->payloadBytes(), $payload->report());
        }
        for ($round = 1; $round <= $this->runs; $round++) {
            foreach ($this->drivers as $driver) {
                if ($result->hasTimedOut($driver)) {
                    continue;
                }
                fwrite(STDERR, "bench: {$workload->value} {$driver->value} run $round/{$this->runs}\n");
                try {
                    $sample = $this->sample($target, $driver, "$directory/{$driver->value}-run.json", Allocator::ZendMm);
                } catch (BudgetExceeded $e) {
                    fwrite(STDERR, "bench: {$e->getMessage()}\n");
                    $result->timedOut($driver);
                    continue;
                }
                $result->timed($driver, $sample[0], $sample[1]);
            }
        }
        foreach ($this->drivers as $driver) {
            if ($result->hasTimedOut($driver)) {
                continue;
            }
            fwrite(STDERR, "bench: {$workload->value} {$driver->value} system allocator (USE_ZEND_ALLOC=0)\n");
            try {
                $sample = $this->sample($target, $driver, "$directory/{$driver->value}-system.json", Allocator::System);
            } catch (BudgetExceeded $e) {
                fwrite(STDERR, "bench: {$e->getMessage()}\n");
                continue;
            }
            $result->systemAllocator($driver, $sample[0], $sample[1]);
        }
        return $result;
    }

    /** @return array{int, Sample} */
    private function sample(Target $target, Driver $driver, string $output, Allocator $allocator): array
    {
        $instrumented = new Instrumented($this->toolchain, $output, $allocator);
        $run = (new Child($this->toolchain, $target, $driver, Mode::Discard))->run($output, $this->budgetSeconds, $instrumented);
        return [$run->tests(), new Sample(
            $run->process->seconds,
            $run->process->peakRssBytes ?? throw new \RuntimeException("no peak RSS for {$target->label()} under {$driver->value}"),
            $run->phpPeakBytes(),
            $instrumented->counters(),
            $instrumented->allocations(),
        )];
    }

    /** @return array<string, string> */
    private function versions(): array
    {
        $versions = [];
        foreach ($this->drivers as $driver) {
            $versions[$driver->value] = $driver->version($this->toolchain);
        }
        return $versions;
    }
}
