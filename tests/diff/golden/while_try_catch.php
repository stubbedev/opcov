<?php

declare(strict_types=1);

final class E extends \Exception
{
}

function g(int $c): int
{
    return $c - 1;
}

function h(E $e): int
{
    return 0;
}

return [
    static fn () => f(2),
];
