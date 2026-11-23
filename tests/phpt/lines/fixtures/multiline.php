<?php
function either(bool $a, bool $b): bool
{
    return $a
        || $b;
}

function pick(int $n): string
{
    return match ($n) {
        1 => 'one',
        2 => 'two',
        default => 'many',
    };
}

either(true, false);
pick(1);
