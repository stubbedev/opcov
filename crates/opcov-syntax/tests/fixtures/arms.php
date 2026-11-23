<?php
function arms($a, $b)
{
    $r = match ($a) {
        1, 2 => 'low',
        default => 'other',
        3 => 'high',
    };
    $s = match (true) {
        $a > 1 => 'big',
        $b => 'b',
    };
    switch ($a) {
        case 1:
        case 2:
            echo 'a';
            break;
        default:
        case 3:
            echo 'b';
        case 4:
    }
    switch (true) {
        case $a === 1:
            echo 'c';
            break;
    }
    switch (false) {
        case $a:
            echo 'd';
            break;
        case $b:
            echo 'e';
    }
    return $r . $s;
}
