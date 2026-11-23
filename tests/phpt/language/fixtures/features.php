<?php
final class Noisy
{
    public function __construct(private string $name)
    {
    }

    public function __destruct()
    {
        if ($this->name === 'unwound') {
            echo "destructed during unwinding\n";
        } else {
            echo "destructed normally\n";
        }
    }
}

function unwind(bool $throw): void
{
    $noisy = new Noisy($throw ? 'unwound' : 'kept');
    if ($throw) {
        throw new RuntimeException('unwinding');
    }
}

function suspended(bool $flag): Generator
{
    if ($flag) {
        yield 'between';
        echo "resumed true\n";
    } else {
        yield 'other';
    }
}

function ticking(int $n): int
{
    declare(ticks=1) {
        $total = 0;
        for ($i = 0; $i < $n; $i++) {
            $total += $i;
        }
    }
    return $total;
}

function includes(string $file, bool $twice): int
{
    $value = include $file;
    if ($twice) {
        $value += include $file;
    }
    return $value;
}

function callables(int $n): array
{
    $double = static fn (int $x): int => $x > 1 ? $x * 2 : $x;
    $length = strlen(...);
    $mapped = array_map($double, range(1, $n));
    return [$mapped, $length('abc')];
}

final class Point
{
    public int $x {
        set(int $value) {
            if ($value < 0) {
                throw new ValueError('negative');
            }
            $this->x = $value;
        }
    }

    public function __construct(int $x)
    {
        echo "initialised\n";
        $this->x = $x;
    }
}

function lazy(bool $touch): string
{
    $reflector = new ReflectionClass(Point::class);
    $point = $reflector->newLazyGhost(static function (Point $ghost): void {
        $ghost->__construct(3);
    });
    if ($touch) {
        return 'x=' . $point->x;
    }
    return 'untouched';
}
