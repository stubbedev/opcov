<?php

declare(strict_types=1);

final class E extends \Exception
{
}

function g(int $i): int
{
    if ($i === 0) {
        throw new E();
    }
    return $i;
}

function h(E $e): void
{
}

return [
    static fn () => f(2),
];
