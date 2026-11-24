<?php

declare(strict_types=1);

namespace Opcov\Measure\Bench;

final readonly class Measured implements Result
{
    public function __construct(
        public int $tests,
        public Sample $sample,
    ) {
    }

    public function covers(int $timeoutMinutes): bool
    {
        return true;
    }

    public function accepts(Step $step): bool
    {
        return $step !== Step::Payload;
    }

    /** @return array{tests: int, sample: array<string, mixed>} */
    public function toArray(): array
    {
        return ['tests' => $this->tests, 'sample' => $this->sample->toArray()];
    }
}
