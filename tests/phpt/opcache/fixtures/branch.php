<?php
function pick(int $n): string
{
    if ($n > 0) {
        return 'positive';
    }
    $unused = $n * 2;
    return 'other';
}
