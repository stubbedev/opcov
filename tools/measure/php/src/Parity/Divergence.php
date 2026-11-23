<?php

declare(strict_types=1);

namespace Opcov\Measure\Parity;

final readonly class Divergence
{
    public function __construct(
        public Aspect $aspect,
        public string $file,
        public int $line,
        public Pair $pair,
        public string $detail,
        public ?Rule $rule = null,
    ) {
    }

    public function key(): string
    {
        return self::keyOf($this->aspect, $this->file, $this->line, $this->pair);
    }

    public static function keyOf(Aspect $aspect, string $file, int $line, Pair $pair): string
    {
        return sprintf('%s %s:%d %s', $aspect->value, $file, $line, $pair->name());
    }

    public function describe(): string
    {
        return sprintf('%s:%d [%s, %s] %s', $this->file, $this->line, $this->aspect->value, $this->pair->name(), $this->detail);
    }
}
