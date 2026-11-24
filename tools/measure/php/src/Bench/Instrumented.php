<?php

declare(strict_types=1);

namespace Opcov\Measure\Bench;

use Opcov\Measure\Toolchain;

final readonly class Instrumented
{
    public function __construct(
        private Toolchain $toolchain,
        private string $output,
        public Allocator $allocator,
    ) {
    }

    /** @return list<string> */
    public function monitor(): array
    {
        return [$this->toolchain->tool('perf'), ...Counters::perfArguments($this->countersPath())];
    }

    /** @return list<string> */
    public function launcher(): array
    {
        $variables = ['LD_PRELOAD' => $this->toolchain->library('alloc-count.so'), 'OPCOV_MEASURE_ALLOC_OUTPUT' => $this->allocationPrefix()] + $this->allocator->environment();
        return [$this->toolchain->tool('env'), ...array_map(static fn (string $name, string $value): string => "$name=$value", array_keys($variables), $variables)];
    }

    public function prepare(): void
    {
        if (is_file($this->countersPath()) && !unlink($this->countersPath())) {
            throw new \RuntimeException("cannot remove the stale counters {$this->countersPath()}");
        }
        Allocations::clear($this->allocationPrefix());
    }

    public function counters(): Counters
    {
        return Counters::read($this->countersPath());
    }

    public function allocations(): Allocations
    {
        return Allocations::collect($this->allocationPrefix());
    }

    private function countersPath(): string
    {
        return "$this->output.perf";
    }

    private function allocationPrefix(): string
    {
        return "$this->output.alloc";
    }
}
