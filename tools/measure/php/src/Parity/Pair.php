<?php

declare(strict_types=1);

namespace Opcov\Measure\Parity;

use Opcov\Measure\Driver;

final readonly class Pair
{
    public function __construct(
        public Driver $left,
        public Driver $right,
    ) {
        if ($left === $right) {
            throw new \InvalidArgumentException("a driver pair needs two different drivers, got {$left->value} twice");
        }
    }

    public function name(): string
    {
        $names = [$this->left->value, $this->right->value];
        sort($names, SORT_STRING);
        return implode('~', $names);
    }

    /** @param list<string> $drivers */
    public static function fromNames(array $drivers, string $context): self
    {
        if (count($drivers) !== 2) {
            throw new \InvalidArgumentException("$context: 'drivers' must name exactly two drivers");
        }
        return new self(Driver::parse($drivers[0]), Driver::parse($drivers[1]));
    }
}
