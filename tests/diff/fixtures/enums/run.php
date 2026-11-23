<?php

declare(strict_types=1);

return [
    static fn () => Suit::fromChar('h')->isRed(),
    static fn () => Suit::fromChar('x')->color(),
];
