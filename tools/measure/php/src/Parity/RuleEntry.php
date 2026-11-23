<?php

declare(strict_types=1);

namespace Opcov\Measure\Parity;

final readonly class RuleEntry
{
    public function __construct(
        public Rule $rule,
        public string $reason,
        public string $issue,
    ) {
    }

    public function describe(): string
    {
        return sprintf('rule %s: %s (%s)', $this->rule->value, $this->reason, $this->issue);
    }
}
