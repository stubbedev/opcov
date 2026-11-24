<?php

declare(strict_types=1);

namespace Opcov\Measure\Bench;

enum Step: string
{
    case Payload = 'payload';
    case Run = 'run';
    case SystemAllocator = 'system-allocator';

    public static function parse(string $name): self
    {
        return self::tryFrom($name) ?? throw new \InvalidArgumentException(sprintf(
            'unknown benchmark step %s; expected one of %s',
            $name,
            implode(', ', array_map(static fn (self $step): string => $step->value, self::cases())),
        ));
    }
}
