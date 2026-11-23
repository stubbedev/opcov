<?php
function sum(array $values): int
{
    $total = 0;
    foreach ($values as $value) {
        $total += $value;
    }
    return $total;
}

function countdown(int $n): void
{
    do {
        $n--;
    } while ($n > 0);
}
