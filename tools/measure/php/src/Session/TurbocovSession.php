<?php

declare(strict_types=1);

namespace Opcov\Measure\Session;

use Opcov\Measure\Coverage;

final class TurbocovSession implements Session
{
    /** @var array<string, true> */
    private array $files = [];

    public function __construct()
    {
        \turbocov\ensure_can_collect();
    }

    public function start(): void
    {
        \turbocov\start();
    }

    public function stop(string $test): mixed
    {
        return \turbocov\stop();
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
        foreach (LineHits::fromXdebugShape($raw, 'turbocov') as $file => $lines) {
            $this->files[$file] = true;
            $coverage->addLines($file, $lines);
        }
    }

    public function writeReport(string $path, string $root): ?int
    {
        return null;
    }

    public function complete(Coverage $coverage): void
    {
        $files = array_keys($this->files);
        foreach (LineHits::fromXdebugShape(\turbocov\get_executable_lines($files), 'turbocov') as $file => $lines) {
            $coverage->addLines($file, $lines);
        }
    }
}
