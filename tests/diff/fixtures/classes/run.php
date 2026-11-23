<?php

declare(strict_types=1);

return [
    static fn () => (new Team(['ada', 'bob']))->greet(),
    static fn () => (new Team())->isEmpty(),
    static fn () => Point::origin()->withX(3),
    static fn () => (new Team(['x']))->logger()->line(),
    static fn () => make('y'),
];
