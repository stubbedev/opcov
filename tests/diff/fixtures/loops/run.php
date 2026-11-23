<?php

declare(strict_types=1);

return [
    static fn () => matrix([[1, 2], [-1, 5], [200]]),
    static fn () => iterate(new Countdown(3)),
    static fn () => iterate([]),
    static fn () => search([1, 3, 5, 7], 7),
    static fn () => retry(2),
    static fn () => jump(3),
];
