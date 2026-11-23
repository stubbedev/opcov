<?php
final class Numbers implements Iterator
{
    public static array $calls = [];
    private int $position = 0;

    public function __construct(private array $values)
    {
    }

    public function current(): mixed
    {
        self::$calls[] = 'current';
        return $this->values[$this->position];
    }

    public function key(): mixed
    {
        self::$calls[] = 'key';
        return $this->position;
    }

    public function next(): void
    {
        self::$calls[] = 'next';
        $this->position++;
    }

    public function rewind(): void
    {
        self::$calls[] = 'rewind';
        $this->position = 0;
    }

    public function valid(): bool
    {
        self::$calls[] = 'valid';
        return isset($this->values[$this->position]);
    }
}

function generated(array $values)
{
    foreach ($values as $value) {
        yield $value;
    }
}

function each_value(iterable $items)
{
    $count = 0;
    foreach ($items as $item) {
        $count++;
    }
    return $count;
}

function each_reference(array $items)
{
    foreach ($items as &$item) {
        $item++;
    }
    return $items;
}

function counted(int $n)
{
    for ($i = 0; $i < $n; $i++) {
        echo '';
    }
}

function waiting(int $n)
{
    while ($n > 0) {
        $n--;
    }
}

function at_least_once(int $n)
{
    do {
        $n--;
    } while ($n > 0);
}

function forever(int $n)
{
    while (true) {
        if ($n-- <= 0) {
            break;
        }
    }
}

function nested(array $rows)
{
    foreach ($rows as $row) {
        foreach ($row as $cell) {
            if ($cell === 'next') {
                continue 2;
            }
            if ($cell === 'stop') {
                break 2;
            }
        }
    }
}

function recursive(int $depth, int $n)
{
    for ($i = 0; $i < $n; $i++) {
        if ($depth > 0) {
            recursive($depth - 1, 0);
        }
    }
}

function yielding(int $n)
{
    for ($i = 0; $i < $n; $i++) {
        yield $i;
    }
}

function in_finally(int $n)
{
    try {
        return 'body';
    } finally {
        for ($i = 0; $i < $n; $i++) {
            echo '';
        }
    }
}

function leave_by_goto(int $n, int $limit)
{
    for ($i = 0; $i < $n; $i++) {
        if ($i === $limit) {
            goto done;
        }
    }
    done:
    return $i;
}
