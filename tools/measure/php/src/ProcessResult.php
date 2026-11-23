<?php

declare(strict_types=1);

namespace Opcov\Measure;

final readonly class ProcessResult
{
    public function __construct(
        public int $exit,
        public float $seconds,
        public ?int $peakRssBytes,
        public string $log,
    ) {
    }

    public function timedOut(): bool
    {
        return $this->exit === Process::TIMED_OUT;
    }

    public function tail(int $lines = 30): string
    {
        $contents = @file($this->log, FILE_IGNORE_NEW_LINES);
        if ($contents === false) {
            return "(no output in $this->log)";
        }
        return implode("\n", array_slice($contents, -$lines));
    }
}
