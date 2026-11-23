<?php
function add(int $a, int $b): int
{
    return $a + $b;
}

function sign(int $n): string
{
    if ($n < 0) {
        return 'negative';
    }
    return 'positive';
}

$double = fn (int $x): int => $x * 2;
