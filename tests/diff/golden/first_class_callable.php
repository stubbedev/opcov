<?php

declare(strict_types=1);

return [
    static fn () => f(new class () {
        public function method(): void
        {
        }
    }, true),
];
