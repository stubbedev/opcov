<?php

declare(strict_types=1);

namespace Opcov\Measure\Bench;

final readonly class Stats
{
    private function __construct(
        public float $median,
        public float $min,
        public float $max,
    ) {
    }

    /** @param list<float|int> $values */
    public static function of(array $values): self
    {
        if ($values === []) {
            throw new \InvalidArgumentException('statistics of no values');
        }
        sort($values);
        $count = count($values);
        $middle = intdiv($count, 2);
        $median = $count % 2 === 1 ? $values[$middle] : ($values[$middle - 1] + $values[$middle]) / 2;
        return new self((float) $median, (float) $values[0], (float) $values[$count - 1]);
    }
}
