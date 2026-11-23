<?php

declare(strict_types=1);

return [
    static fn () => render(['name' => 'ada']),
    static fn () => compute(1, 2),
    static fn () => call('a; b'),
];
