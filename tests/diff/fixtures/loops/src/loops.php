<?php

declare(strict_types=1);

final class Countdown implements Iterator
{
    private int $current;

    public function __construct(private readonly int $from)
    {
        $this->current = $from;
    }

    public function current(): int
    {
        return $this->current;
    }

    public function key(): int
    {
        return $this->from - $this->current;
    }

    public function next(): void
    {
        $this->current--;
    }

    public function rewind(): void
    {
        $this->current = $this->from;
    }

    public function valid(): bool
    {
        return $this->current > 0;
    }
}

function matrix(array $rows): int
{
    $sum = 0;
    foreach ($rows as $row) {
        foreach ($row as $cell) {
            if ($cell < 0) {
                continue 2;
            }
            if ($cell > 100) {
                break 2;
            }
            $sum += $cell;
        }
    }
    return $sum;
}

function iterate(iterable $items): int
{
    $total = 0;
    foreach ($items as $item) {
        $total += $item;
    }
    return $total;
}

function search(array $haystack, int $needle): int
{
    $low = 0;
    $high = count($haystack) - 1;
    while ($low <= $high) {
        $middle = intdiv($low + $high, 2);
        if ($haystack[$middle] === $needle) {
            return $middle;
        } elseif ($haystack[$middle] < $needle) {
            $low = $middle + 1;
        } else {
            $high = $middle - 1;
        }
    }
    return -1;
}

function retry(int $attempts): int
{
    $tries = 0;
    do {
        $tries++;
    } while ($tries < $attempts && $tries < 5);
    return $tries;
}

function jump(int $n): int
{
    $i = 0;
    start:
    $i++;
    if ($i < $n) {
        goto start;
    }
    return $i;
}
