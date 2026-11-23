<?php

declare(strict_types=1);

return [
    static fn () => grade(85),
    static fn () => grade(10),
    static fn () => verb('POST'),
    static function (): void {
        try {
            verb('TRACE');
        } catch (UnhandledMatchError) {
        }
    },
    static fn () => multiline(1),
];
