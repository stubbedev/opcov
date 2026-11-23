<?php

declare(strict_types=1);

namespace Opcov\Measure\Parity;

final readonly class Verdict
{
    /**
     * @param list<array{0: Divergence, 1: AllowlistEntry|RuleEntry}> $explained
     * @param list<Divergence> $unexplained
     * @param list<AllowlistEntry|RuleEntry> $stale
     */
    public function __construct(
        public array $explained,
        public array $unexplained,
        public array $stale,
    ) {
    }

    public function passed(): bool
    {
        return $this->unexplained === [] && $this->stale === [];
    }
}
