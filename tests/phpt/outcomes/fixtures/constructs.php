<?php
function cond($a)
{
    if ($a) {
        return 'then';
    }
    return 'implicit else';
}

function chain($a)
{
    if ($a > 1) {
        return 'big';
    } elseif ($a) {
        return 'one';
    } else {
        return 'zero';
    }
}

function logic($a, $b)
{
    return ($a && $b) || $a;
}

function words($a, $b)
{
    return ($a and $b) or $b;
}

function coalesce($a)
{
    return $a ?? 'fallback';
}

function coalesce_assign(array $a)
{
    $a['k'] ??= 'fallback';
    return $a['k'];
}

function ternary($a)
{
    return $a ?: 'fallback';
}

function nullsafe($o)
{
    return $o?->p;
}

function table($a)
{
    return match ($a) {
        1, 2 => 'low',
        3 => 'mid',
        default => 'high',
    };
}

function conditions($a)
{
    return match (true) {
        $a > 1 => 'big',
        default => 'small',
    };
}

function no_arm($a)
{
    return match ($a) {
        1 => 'one',
        2 => 'two',
    };
}

function cases($a)
{
    switch ($a) {
        case 1:
            return 'one';
        case 2:
            return 'two';
        default:
            return 'many';
    }
}

function caught(callable $f)
{
    try {
        $f();
        return 'none';
    } catch (LogicException $e) {
        return 'logic';
    } catch (RuntimeException $e) {
        return 'runtime';
    }
}

function finished(callable $f)
{
    try {
        $f();
    } finally {
        echo "finally\n";
    }
}

function one_line($a, $b)
{
    return [$a ?? 1, $b ?? 2, $a ?: 3];
}
