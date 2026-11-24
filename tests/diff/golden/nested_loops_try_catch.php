<?php

declare(strict_types=1);

final class E extends \Exception
{
    /** @param list<int> $items */
    public function __construct(public readonly array $items)
    {
        parent::__construct();
    }
}

function g(int $i): int
{
    if ($i === 0) {
        throw new E([1]);
    }
    return $i;
}

function h(int $item): void
{
}

return [
    static fn () => f(2),
];
