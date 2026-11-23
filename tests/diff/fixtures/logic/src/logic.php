<?php

declare(strict_types=1);

function eligible(?int $age, bool $member, ?string $code): bool
{
    $discount = $code ?? 'none';
    $settings = [];
    $settings['tier'] ??= 'basic';
    if (($age !== null && $age >= 18) || $member) {
        return $discount !== 'none' and $settings['tier'] === 'basic';
    }
    return $member xor ($age ?: 0) > 65;
}

function describe(mixed $value): string
{
    $kind = is_int($value) ? 'int' : (is_string($value) ? 'string' : 'other');
    $text = $value ?: 'empty';
    return "$kind:" . (is_scalar($text) ? $text : 'complex');
}

function longCondition(int $a, int $b, int $c): bool
{
    return $a > 0
        && $b > 0
        && ($c > 0
            || $a + $b > 10);
}

function switcher(string $mode): int
{
    switch ($mode) {
        case 'fast':
            return 1;
        case 'slow':
        case 'careful':
            $result = 2;
            break;
        default:
            $result = 3;
    }
    return $result;
}

function numbers(int $n): string
{
    switch (true) {
        case $n < 0:
            return 'negative';
        case $n === 0:
            return 'zero';
    }
    return 'positive';
}
