<?php

declare(strict_types=1);

return [
    static fn () => f(4, 5),
    static function (): void {
        try {
            f(9, 5);
        } catch (\UnhandledMatchError) {
        }
    },
];
