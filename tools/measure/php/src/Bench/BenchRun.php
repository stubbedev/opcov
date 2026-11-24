<?php

declare(strict_types=1);

namespace Opcov\Measure\Bench;

use Opcov\Measure\Arguments;
use Opcov\Measure\BudgetExceeded;
use Opcov\Measure\Child;
use Opcov\Measure\Cli;
use Opcov\Measure\DocSection;
use Opcov\Measure\Driver;
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
        private Build $build,
        private array $workloads,
        private array $drivers,
        private int $runs,
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
            Build::of($toolchain),
            $workloads,
            $drivers,
            $arguments->positiveInt('--runs', 3),
            $arguments->flag('--write-docs'),
            $arguments->flag('--check'),
            $arguments->flag('--update-baseline'),
        );
    }

    public function measure(int $timeoutMinutes, bool $fresh): void
    {
        $log = SampleLog::open($this->samplesPath());
        if ($fresh) {
            $discarded = $log->discard(fn (Record $record): bool => in_array($record->key->workload, $this->workloads, true)
                && in_array($record->key->driver, $this->drivers, true)
                && $record->build === $this->build->identity($record->key->driver));
            fwrite(STDERR, "bench: --fresh discarded $discarded recorded samples of this build\n");
        }
        $machine = Machine::describe();
        foreach ($this->workloads as $workload) {
            $this->measureWorkload($workload, $log, $machine, $timeoutMinutes);
        }
    }

    public function report(): int
    {
        $log = SampleLog::open($this->samplesPath());
        $earliest = null;
        $lookup = function (Key $key) use ($log, &$earliest): Record {
            $record = $log->find($key, $this->build->identity($key->driver)) ?? throw new \RuntimeException(sprintf(
                '%s has no %s for this build; measure it with tools/measure/bench --workload %s --driver %s --runs %d',
                $log->path,
                $key->label(),
                $key->workload->value,
                $key->driver->value,
                $this->runs,
            ));
            if ($earliest === null || strcmp($record->measuredAt, $earliest->measuredAt) < 0) {
                $earliest = $record;
            }
            return $record;
        };
        $results = [];
        foreach ($this->workloads as $workload) {
            $results[$workload->value] = WorkloadResult::collect($workload, $this->drivers, $this->runs, $lookup);
        }
        if (!$earliest instanceof Record) {
            throw new \LogicException('the report selected no samples');
        }
        $report = new BenchReport($this->toolchain, $this->versions(), $earliest->machine, $this->runs, $results);
        echo $report->text();
        if ($this->writeDocs) {
            DocSection::replace("{$this->toolchain->root}/docs/benchmarks.md", 'bench:php' . str_replace('.', '', $this->toolchain->phpVersion()), $report->markdown());
        }
        $baseline = new Baseline("{$this->toolchain->root}/bench/baseline.json");
        if ($this->updateBaseline) {
            $baseline->store($this->toolchain->phpVersion(), $report);
        }
        if (!$this->check) {
            return Cli::PASSED;
        }
        foreach ($baseline->warnings($this->toolchain->phpVersion(), $report, self::REGRESSION_TOLERANCE) as $warning) {
            fwrite(STDERR, "bench: warning (cycles depend on machine load): $warning\n");
        }
        $regressions = $baseline->regressions($this->toolchain->phpVersion(), $report, self::REGRESSION_TOLERANCE);
        foreach ($regressions as $regression) {
            fwrite(STDERR, "bench: regression: $regression\n");
        }
        return $regressions === [] ? Cli::PASSED : Cli::FAILED;
    }

    /** @param array<string, string> $machine */
    private function measureWorkload(BenchWorkload $workload, SampleLog $log, array $machine, int $timeoutMinutes): void
    {
        $target = null;
        $prepared = function () use ($workload, &$target): Target {
            return $target ??= $workload->target($this->toolchain);
        };
        $directory = sprintf('%s/bench/%s/%s', $this->toolchain->cache, $this->toolchain->phpVersion(), $workload->value);
        $seconds = $timeoutMinutes * 60;
        $record = function (Key $key, \Closure $measure) use ($log, $machine, $timeoutMinutes): void {
            $build = $this->build->identity($key->driver);
            $recorded = $log->find($key, $build);
            if ($recorded !== null && $recorded->result->covers($timeoutMinutes)) {
                fwrite(STDERR, "bench: {$key->label()}: kept the sample measured at $recorded->measuredAt\n");
                return;
            }
            fwrite(STDERR, "bench: {$key->label()}\n");
            try {
                $result = $measure();
            } catch (BudgetExceeded $e) {
                fwrite(STDERR, "bench: {$e->getMessage()}\n");
                $result = new TimedOut($timeoutMinutes);
            }
            $log->append(new Record($key, $build, gmdate('Y-m-d\TH:i:s\Z'), $machine, $result));
        };
        foreach ($this->drivers as $driver) {
            $record(Key::payload($workload, $driver), function () use ($prepared, $driver, $directory, $seconds): PayloadSize {
                $payload = (new Child($this->toolchain, $prepared(), $driver, Mode::Payload))->run("$directory/{$driver->value}-payload.json", $seconds);
                return new PayloadSize($payload->tests(), $payload->payloadBytes(), $payload->report());
            });
        }
        for ($round = 1; $round <= $this->runs; $round++) {
            foreach ($this->drivers as $driver) {
                if ($this->stopped($log, $workload, $driver, $timeoutMinutes)) {
                    continue;
                }
                $record(Key::run($workload, $driver, $round), fn (): Measured => $this->sample($prepared(), $driver, "$directory/{$driver->value}-run.json", Allocator::ZendMm, $seconds));
            }
        }
        foreach ($this->drivers as $driver) {
            if ($this->stopped($log, $workload, $driver, $timeoutMinutes)) {
                continue;
            }
            $record(Key::systemAllocator($workload, $driver), fn (): Measured => $this->sample($prepared(), $driver, "$directory/{$driver->value}-system.json", Allocator::System, $seconds));
        }
    }

    private function stopped(SampleLog $log, BenchWorkload $workload, Driver $driver, int $timeoutMinutes): bool
    {
        $build = $this->build->identity($driver);
        $keys = [Key::payload($workload, $driver)];
        for ($round = 1; $round <= $this->runs; $round++) {
            $keys[] = Key::run($workload, $driver, $round);
        }
        foreach ($keys as $key) {
            $result = $log->find($key, $build)?->result;
            if ($result instanceof TimedOut && $result->covers($timeoutMinutes)) {
                return true;
            }
        }
        return false;
    }

    private function sample(Target $target, Driver $driver, string $output, Allocator $allocator, int $seconds): Measured
    {
        $instrumented = new Instrumented($this->toolchain, $output, $allocator);
        $run = (new Child($this->toolchain, $target, $driver, Mode::Discard))->run($output, $seconds, $instrumented);
        return new Measured($run->tests(), new Sample(
            $run->process->seconds,
            $run->process->peakRssBytes ?? throw new \RuntimeException("no peak RSS for {$target->label()} under {$driver->value}"),
            $run->phpPeakBytes(),
            $instrumented->counters(),
            $instrumented->allocations(),
        ));
    }

    private function samplesPath(): string
    {
        return sprintf('%s/bench/%s/samples.jsonl', $this->toolchain->cache, $this->toolchain->phpVersion());
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
