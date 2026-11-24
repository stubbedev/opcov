<?php

declare(strict_types=1);

return [
    static fn () => sum(new \ArrayIterator([2 => 3])),
    static fn () => sum(new \ArrayIterator([])),
];
