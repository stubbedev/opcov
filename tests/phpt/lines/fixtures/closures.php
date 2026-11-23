<?php
$squares = [];
foreach ([1, 2, 3] as $n) {
    $squares[] = function () use ($n) {
        return $n * $n;
    };
}
$squares[0]();
$double = fn (int $x) => $x * 2;
$double(2);
