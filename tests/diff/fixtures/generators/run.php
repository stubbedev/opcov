<?php

declare(strict_types=1);

return [
    static fn () => consume(delegate(3)),
    static fn () => consume(numbers(0)),
    static function (): void {
        foreach (lazy() as $value) {
            break;
        }
    },
];
