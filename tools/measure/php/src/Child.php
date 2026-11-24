<?php

declare(strict_types=1);

namespace Opcov\Measure;

use Opcov\Measure\Bench\Instrumented;

final readonly class Child
{
    public function __construct(
        private Toolchain $toolchain,
        private Target $target,
        private Driver $driver,
        private Mode $mode,
    ) {
    }

    public function run(string $output, int $budgetSeconds, ?Instrumented $instrumented = null): ChildResult
    {
        $instrumented?->prepare();
        $config = new Config($this->driver, $this->mode, $this->target->selection(), $this->target->root(), $output);
        if (is_file($output) && !unlink($output)) {
            throw new \RuntimeException("cannot remove the stale result $output");
        }
        $process = Process::run(
            $this->toolchain,
            [...$instrumented?->launcher() ?? [], $this->toolchain->php(), ...$this->driver->iniArguments($this->toolchain, $this->target->selection()), ...$this->target->script()],
            $config->environment() + $this->target->environment(),
            $this->target->cwd(),
            "$output.log",
            $budgetSeconds,
            $instrumented?->monitor() ?? [],
        );
        $context = sprintf('%s under %s', $this->target->label(), $this->driver->value);
        if ($process->timedOut()) {
            throw new BudgetExceeded("$context exceeded its budget of {$budgetSeconds}s; log: $process->log");
        }
        if (!in_array($process->exit, $this->target->acceptedExits(), true)) {
            throw new \RuntimeException("$context exited with $process->exit; last output:\n" . $process->tail());
        }
        if (!is_file($output)) {
            throw new \RuntimeException("$context exited with $process->exit and wrote no result; last output:\n" . $process->tail());
        }
        $result = Json::read($output);
        if (!is_int($result['tests'] ?? null)) {
            throw new \RuntimeException("$output lacks the test count");
        }
        return new ChildResult($process, $result);
    }
}
