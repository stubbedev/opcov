<?php

declare(strict_types=1);

namespace Opcov\Measure\Session;

use Opcov\Measure\Coverage;
use Opcov\Measure\ReportSize;

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

    public function writeReport(string $path, string $root): ReportSize
    {
        return ReportSize::none();
    }

    public function complete(Coverage $coverage): void
    {
    }
}
