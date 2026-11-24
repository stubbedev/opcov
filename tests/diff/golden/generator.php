<?php

declare(strict_types=1);

return [
    static fn () => iterator_to_array(gen([1, 0]), false),
];
