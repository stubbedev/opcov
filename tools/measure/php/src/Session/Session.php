<?php

declare(strict_types=1);

namespace Opcov\Measure\Session;

use Opcov\Measure\Coverage;
use Opcov\Measure\ReportSize;

interface Session
{
    public function start(): void;

    public function stop(string $test): mixed;

    public function rearm(): void;

    public function collect(string $test, Coverage $coverage): void;

    public function writeReport(string $path, string $root): ReportSize;

    public function complete(Coverage $coverage): void;
}
