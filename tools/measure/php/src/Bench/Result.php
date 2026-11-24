<?php

declare(strict_types=1);

namespace Opcov\Measure\Bench;

interface Result
{
    public function covers(int $timeoutMinutes): bool;

    public function accepts(Step $step): bool;

    /** @return array<string, mixed> */
    public function toArray(): array;
}
