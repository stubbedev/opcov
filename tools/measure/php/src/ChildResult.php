<?php

declare(strict_types=1);

namespace Opcov\Measure;

final readonly class ChildResult
{
    /** @param array<mixed> $data */
    public function __construct(
        public ProcessResult $process,
        public array $data,
    ) {
    }

    public function tests(): int
    {
        return $this->data['tests'];
    }

    public function coverage(string $source): Coverage
    {
        return Coverage::fromArray($this->data, $source);
    }

    public function reportBytes(): ?int
    {
        $bytes = $this->data['report_bytes'] ?? null;
        return is_int($bytes) || $bytes === null ? $bytes : throw new \RuntimeException("{$this->process->log}: malformed report size");
    }

    public function payloadBytes(): int
    {
        return is_int($this->data['payload_bytes'] ?? null) ? $this->data['payload_bytes'] : throw new \RuntimeException("{$this->process->log}: the run reported no payload size");
    }
}
