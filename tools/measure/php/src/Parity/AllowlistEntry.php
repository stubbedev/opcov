<?php

declare(strict_types=1);

namespace Opcov\Measure\Parity;

final readonly class AllowlistEntry
{
    public function __construct(
        public Aspect $aspect,
        public string $file,
        public int $line,
        public Pair $pair,
        public string $reason,
        public string $issue,
    ) {
    }

    public function key(): string
    {
        return Divergence::keyOf($this->aspect, $this->file, $this->line, $this->pair);
    }

    /** @param list<string> $labels */
    public function coveredBy(array $labels): bool
    {
        foreach ($labels as $label) {
            if (str_starts_with($this->file, "$label/")) {
                return true;
            }
        }
        return false;
    }

    public function describe(): string
    {
        return sprintf('%s:%d [%s, %s] %s (%s)', $this->file, $this->line, $this->aspect->value, $this->pair->name(), $this->reason, $this->issue);
    }
}
