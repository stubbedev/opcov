<?php

declare(strict_types=1);

namespace Opcov\Measure\Bench;

use Opcov\Measure\Driver;

final readonly class Key
{
    private function __construct(
        public BenchWorkload $workload,
        public Driver $driver,
        public Step $step,
        public ?int $round,
    ) {
    }

    public static function payload(BenchWorkload $workload, Driver $driver): self
    {
        return new self($workload, $driver, Step::Payload, null);
    }

    public static function run(BenchWorkload $workload, Driver $driver, int $round): self
    {
        if ($round < 1) {
            throw new \InvalidArgumentException("benchmark round $round of {$workload->value} under {$driver->value} is not positive");
        }
        return new self($workload, $driver, Step::Run, $round);
    }

    public static function systemAllocator(BenchWorkload $workload, Driver $driver): self
    {
        return new self($workload, $driver, Step::SystemAllocator, null);
    }

    /** @param array<mixed> $data */
    public static function fromArray(array $data): self
    {
        $workload = BenchWorkload::parse(is_string($data['workload'] ?? null) ? $data['workload'] : throw new \RuntimeException('no workload'));
        $driver = Driver::parse(is_string($data['driver'] ?? null) ? $data['driver'] : throw new \RuntimeException('no driver'));
        $step = Step::parse(is_string($data['step'] ?? null) ? $data['step'] : throw new \RuntimeException('no step'));
        return match ($step) {
            Step::Payload => self::payload($workload, $driver),
            Step::Run => self::run($workload, $driver, is_int($data['round'] ?? null) ? $data['round'] : throw new \RuntimeException('a run has no integer round')),
            Step::SystemAllocator => self::systemAllocator($workload, $driver),
        };
    }

    /** @return array{workload: string, driver: string, step: string, round?: int} */
    public function toArray(): array
    {
        $key = ['workload' => $this->workload->value, 'driver' => $this->driver->value, 'step' => $this->step->value];
        return $this->round === null ? $key : $key + ['round' => $this->round];
    }

    public function label(): string
    {
        $step = match ($this->step) {
            Step::Payload => 'payload',
            Step::Run => "run $this->round",
            Step::SystemAllocator => 'system allocator (USE_ZEND_ALLOC=0)',
        };
        return "{$this->workload->value} {$this->driver->value} $step";
    }
}
