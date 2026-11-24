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

    /** @param array<mixed> $data */
    public static function fromArray(array $data): self
    {
        $seconds = $data['seconds'] ?? null;
        return new self(
            is_int($seconds) || is_float($seconds) ? (float) $seconds : throw new \RuntimeException('no number seconds'),
            is_int($data['peak_rss_bytes'] ?? null) ? $data['peak_rss_bytes'] : throw new \RuntimeException('no integer peak_rss_bytes'),
            is_int($data['php_peak_bytes'] ?? null) ? $data['php_peak_bytes'] : throw new \RuntimeException('no integer php_peak_bytes'),
            Counters::fromArray(is_array($data['counters'] ?? null) ? $data['counters'] : throw new \RuntimeException('counters is not an object')),
            Allocations::fromArray(is_array($data['allocations'] ?? null) ? $data['allocations'] : throw new \RuntimeException('allocations is not an object')),
        );
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
