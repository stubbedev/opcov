<?php

declare(strict_types=1);

return [
    static fn () => f(null),
    static fn () => f((object) ["b" => (object) ["c" => 1]]),
];
