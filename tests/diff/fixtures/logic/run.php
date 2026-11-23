<?php

declare(strict_types=1);

return [
    static fn () => eligible(20, false, null),
    static fn () => eligible(null, false, 'x'),
    static fn () => describe(0),
    static fn () => describe('a'),
    static fn () => longCondition(1, 1, 0),
    static fn () => switcher('slow'),
    static fn () => switcher('other'),
    static fn () => numbers(0),
];
