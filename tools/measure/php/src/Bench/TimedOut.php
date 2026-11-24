<?php

declare(strict_types=1);

namespace Opcov\Measure\Bench;

final readonly class TimedOut implements Result
{
    public function __construct(public int $minutes)
    {
        if ($minutes < 1) {
            throw new \InvalidArgumentException("a timeout of $minutes minutes is not positive");
        }
    }

    public function covers(int $timeoutMinutes): bool
    {
        return $this->minutes >= $timeoutMinutes;
    }

    public function accepts(Step $step): bool
    {
        return true;
    }

    public function cell(): string
    {
        return "> $this->minutes min";
    }

    /** @return array{timed_out_after_minutes: int} */
    public function toArray(): array
    {
        return ['timed_out_after_minutes' => $this->minutes];
    }
}
