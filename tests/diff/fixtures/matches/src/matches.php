<?php

declare(strict_types=1);

function grade(int $score): string
{
    return match (true) {
        $score >= 90 => 'A',
        $score >= 80 => 'B',
        $score >= 70 => 'C',
        default => 'F',
    };
}

function verb(string $method): string
{
    return match ($method) {
        'GET', 'HEAD' => 'read',
        'POST' => 'create',
        'PUT', 'PATCH' => 'update',
        'DELETE' => 'delete',
    };
}

function multiline(int $value): string
{
    $result = match ($value) {
        1 => 'one',
        2 => strtoupper(
            'two'
        ),
        default => sprintf(
            '%d',
            $value,
        ),
    };
    return $result;
}
