<?php

declare(strict_types=1);

function render(array $user): string
{
    $name = $user['name'] ?? 'guest';
    $html = <<<HTML
        <p>
            {$name}
        </p>
        HTML;
    $extra = isset($user['admin'])
        ? sprintf(
            '<b>%s</b>',
            'admin',
        )
        : '';
    return $html . $extra;
}

function compute(int $a, int $b): int
{
    $result = $a
        + $b
        * 2;
    return $result
        - 1;
}

function call(string $value): string
{
    return implode(
        ',',
        array_map(
            static fn (string $part): string => trim($part),
            explode(';', $value),
        ),
    );
}
