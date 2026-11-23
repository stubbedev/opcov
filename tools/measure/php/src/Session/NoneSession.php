<?php

declare(strict_types=1);

namespace Opcov\Measure\Session;

use Opcov\Measure\Coverage;

final class NoneSession implements Session
{
    public function start(): void
    {
    }

    public function stop(string $test): mixed
    {
        return null;
    }

    public function rearm(): void
    {
    }

    public function collect(string $test, Coverage $coverage): void
    {
    }

    public function writeReport(string $path, string $root): ?int
    {
        return null;
    }

    public function complete(Coverage $coverage): void
    {
    }
}
