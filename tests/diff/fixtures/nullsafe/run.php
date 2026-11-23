<?php

declare(strict_types=1);

return [
    static fn () => city(null),
    static fn () => city(new Person(new Address('oslo'))),
    static fn () => cityLength(new Person()),
];
