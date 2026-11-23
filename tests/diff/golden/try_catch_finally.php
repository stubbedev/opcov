<?php

declare(strict_types=1);

return [
    static fn () => f(static fn () => null),
    static fn () => f(static fn () => throw new \RuntimeException("x")),
];
