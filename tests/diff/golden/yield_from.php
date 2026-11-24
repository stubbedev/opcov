<?php

declare(strict_types=1);

return [
    static fn () => iterator_to_array(delegate((static function (): \Generator {
        yield 1;
        return 5;
    })(), false), false),
];
