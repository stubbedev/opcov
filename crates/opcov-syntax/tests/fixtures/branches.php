<?php
function branches($a, $b, $c)
{
    if ($a) {
        echo 1;
    } elseif ($b && $c) {
        echo 2;
    } else {
        echo 3;
    }
    if ($a || $b) {
        echo 4;
    }
    $x = $a ? $b : $c;
    $y = $a ?: $b;
    $z = $a ?? $b ?? $c;
    $b ??= $c;
    $w = $a?->b?->c();
    $v = (($a and $b) or $c);
    $u = $a
        ? ($b ? 1 : 2)
        : 3;
    return $x . $y . $z . $w . $v . $u;
}
