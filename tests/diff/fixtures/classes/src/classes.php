<?php

declare(strict_types=1);

trait Greets
{
    public function greet(): string
    {
        return 'Hello, ' . $this->name();
    }

    abstract protected function name(): string;
}

abstract class Base
{
    abstract public function size(): int;

    public function isEmpty(): bool
    {
        return $this->size() === 0;
    }
}

final readonly class Point
{
    public function __construct(
        public int $x,
        public int $y,
    ) {
    }

    public function withX(int $x): self
    {
        return new self($x, $this->y);
    }

    public static function origin(): self
    {
        static $origin = null;
        return $origin ??= new self(0, 0);
    }
}

final class Team extends Base
{
    use Greets;

    /** @param list<string> $members */
    public function __construct(private array $members = [])
    {
    }

    public function size(): int
    {
        return count($this->members);
    }

    protected function name(): string
    {
        return $this->members[0] ?? 'nobody';
    }

    public function logger(): object
    {
        return new class ($this->size()) {
            public function __construct(private int $size)
            {
            }

            public function line(): string
            {
                return $this->size > 1 ? 'team' : 'solo';
            }
        };
    }
}

function make(string $name): object
{
    $instance = new Team(
        [$name],
    );
    return $instance;
}
