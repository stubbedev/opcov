<?php

declare(strict_types=1);

function numbers(int $limit): Generator
{
    for ($i = 0; $i < $limit; $i++) {
        if ($i % 2 === 0) {
            yield $i => 'even';
        } else {
            yield $i => 'odd';
        }
    }
    return $limit;
}

function delegate(int $limit): Generator
{
    $returned = yield from numbers($limit);
    yield 'returned' => $returned;
}

function consume(iterable $values): array
{
    $seen = [];
    foreach ($values as $key => $value) {
        $seen[] = "$key:$value";
    }
    return $seen;
}

function lazy(): Generator
{
    try {
        yield 1;
        yield 2;
    } finally {
        echo 'cleanup';
    }
}
