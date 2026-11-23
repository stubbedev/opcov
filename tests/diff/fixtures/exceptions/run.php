<?php

declare(strict_types=1);

return [
    static fn () => attempt(static fn () => 42),
    static fn () => attempt(static fn () => throw new DomainException('d')),
    static fn () => finallyReturns(true),
    static fn () => nested(2),
];
