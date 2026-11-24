<?php

declare(strict_types=1);

namespace Opcov\Measure\Bench;

final readonly class Sample
{
    public function __construct(
        public float $seconds,
        public int $peakRssBytes,
        public int $phpPeakBytes,
        public Counters $counters,
        public Allocations $allocations,
    ) {
    }

    /** @return array<string, mixed> */
    public function toArray(): array
    {
        return [
            'seconds' => $this->seconds,
            'peak_rss_bytes' => $this->peakRssBytes,
            'php_peak_bytes' => $this->phpPeakBytes,
            'counters' => $this->counters->toArray(),
            'allocations' => $this->allocations->toArray(),
        ];
    }
}
