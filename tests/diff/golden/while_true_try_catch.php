<?php

declare(strict_types=1);

final class E extends \Exception
{
}

function g(): int
{
    static $calls = 0;
    if ($calls++ === 0) {
        throw new E();
    }
    return 1;
}

function h(E $e): void
{
}

return [
    static fn () => f(),
];
