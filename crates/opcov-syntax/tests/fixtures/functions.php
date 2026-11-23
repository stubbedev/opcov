<?php
namespace App\Models;

use Attribute;

#[Attribute]
final class User
{
    public string $email {
        get => strtolower($this->email);
        set(string $value) {
            $this->email = $value ?: 'none';
        }
    }

    public function __construct(
        public int $age = 1 ?? 2,
        public string $name { set => trim($value); },
    ) {
    }

    public function filter(array $items): array
    {
        $double = array_map(fn ($x) => $x * 2, array_filter($items, fn ($x) => $x > 0));
        $nested = fn ($a) => fn ($b) => $a ?? $b;
        $closure = static function () use ($items) {
            return $items ?: [];
        };
        $multiline = array_map(fn ($x) => $x
            ->value(), $items);
        return [$double, $nested, $closure, $multiline];
    }
}

interface Shape
{
    public function area(): float;
}

trait Greets
{
    public function greet(): string
    {
        return $this->name ?? 'anonymous';
    }
}

enum Suit: string
{
    case Hearts = 'H';

    public function color(): string
    {
        return $this === self::Hearts ? 'Red' : 'Black';
    }
}

function helper(int $x = PHP_INT_MAX ?? 0): int
{
    return $x;
}

$first = new class { public function run() { return 1 ?: 2; } }; $second = new class { public function run() { return 2; } };
