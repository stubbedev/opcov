<?php
function columns($a, $b)
{
    $text = <<<TXT
    héllo {$a}
    TXT; $x = $text ?? $b;
    $raw = <<<'RAW'
    ünïcödé
    RAW; $y = $raw ?: $a;
    $z = 'ü€' . ($a ?? 'ß') . ($b ?? 'x');
    return ($a |> strlen(...)) ?? $x;
}
