<?php

declare(strict_types=1);

namespace Opcov\Measure\Gate;

final readonly class FunctionSpan
{
    public function __construct(
        public string $key,
        public int $start,
        public int $end,
        public bool $nested,
    ) {
    }

    public function contains(int $line): bool
    {
        return $this->start <= $line && $line <= $this->end;
    }

    public function containsGapBefore(int $line): bool
    {
        return $this->start < $line && $line <= $this->end;
    }

    public function encloses(self $inner): bool
    {
        return $this !== $inner && $this->start <= $inner->start && $inner->end <= $this->end;
    }

    public function narrowerThan(self $other): bool
    {
        return [$this->start, -$this->end] > [$other->start, -$other->end];
    }
}
