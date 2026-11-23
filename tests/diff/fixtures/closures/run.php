<?php

declare(strict_types=1);

return [
    static fn () => (new Pipeline())->pipe(multiplier(2))->pipe(adder(1))->pipe(abs(...))->run(21),
    static function (): void {
        $next = counter();
        $next();
        $next();
    },
];
