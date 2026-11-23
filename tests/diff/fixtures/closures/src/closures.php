<?php

declare(strict_types=1);

final class Pipeline
{
    /** @var list<Closure> */
    private array $stages = [];

    public function pipe(callable $stage): self
    {
        $this->stages[] = $stage(...);
        return $this;
    }

    public function run(int $value): int
    {
        return array_reduce(
            $this->stages,
            static fn (int $carry, Closure $stage): int => $stage($carry),
            $value,
        );
    }
}

function multiplier(int $factor): Closure
{
    return static function (int $value) use ($factor): int {
        if ($factor === 0) {
            return 0;
        }
        return $value * $factor;
    };
}

function adder(int $amount): Closure
{
    return fn (int $value): int => $value + $amount;
}

function counter(): Closure
{
    $count = 0;
    return function () use (&$count): int {
        return ++$count;
    };
}
