<?php

declare(strict_types=1);

namespace Opcov\Measure\Parity;

enum Rule: string
{
    case DeclarationsOnlyReturn = 'declarations-only-return';

    public function describe(): string
    {
        return match ($this) {
            self::DeclarationsOnlyReturn => 'opcov reports no line where PCOV and Xdebug report the implicit `return` at the end of a file',
        };
    }
}
