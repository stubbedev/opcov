<?php

namespace App;

class Foo
{
    public function bar(?array $items, ?string $name): string
    {
        $label = $name ?? 'none';
        if ($items === null) {
            return $label;
        }
        foreach ($items as $item) {
            $label .= $item;
        }
        return $label;
    }

    public function baz(int $x): int
    {
        return $x * 2;
    }
}

function helper(): int
{
    $f = fn (int $y): int => $y + 1;
    return $f(1);
}
