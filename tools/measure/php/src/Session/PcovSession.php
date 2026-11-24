<?php

declare(strict_types=1);

namespace Opcov\Measure\Session;

use Opcov\Measure\Coverage;
use Opcov\Measure\ReportSize;

final class PcovSession implements Session
{
    public function start(): void
    {
        \pcov\start();
    }

    public function stop(string $test): mixed
    {
        \pcov\stop();
        $waiting = \pcov\waiting();
        if ($waiting === []) {
            return [];
        }
        $collected = \pcov\collect(\pcov\inclusive, $waiting);
        \pcov\clear();
        return $collected;
    }

    public function rearm(): void
    {
    }

    public function collect(string $test, Coverage $coverage): void
    {
        $this->record($this->stop($test), $coverage);
    }

    private function record(mixed $raw, Coverage $coverage): void
    {
        foreach (LineHits::fromXdebugShape($raw, 'PCOV') as $file => $lines) {
            $coverage->addLines($file, $lines);
        }
    }

    public function writeReport(string $path, string $root): ReportSize
    {
        return ReportSize::none();
    }

    public function complete(Coverage $coverage): void
    {
    }
}
