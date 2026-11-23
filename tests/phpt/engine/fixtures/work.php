<?php
function work(int $n): int
{
    $total = 0;
    for ($i = 0; $i < $n; $i++) {
        $total += $i;
    }
    return $total;
}
