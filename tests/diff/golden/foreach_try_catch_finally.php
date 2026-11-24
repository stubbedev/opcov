<?php

declare(strict_types=1);

final class E extends \Exception
{
}

function g(int $item): void
{
    if ($item < 0) {
        throw new E();
    }
}

function h(E $e): void
{
}

function k(int $item): void
{
}

return [
    static fn () => f([1, -1]),
];
