<?php

declare(strict_types=1);

namespace Opcov\Measure\Bench;

enum Allocator: string
{
    case ZendMm = 'zend-mm';
    case System = 'system';

    /** @return array<string, string> */
    public function environment(): array
    {
        return match ($this) {
            self::ZendMm => [],
            self::System => ['USE_ZEND_ALLOC' => '0'],
        };
    }
}
