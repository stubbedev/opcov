<?php

declare(strict_types=1);

namespace Opcov\Measure;

final readonly class Loop
{
    public function __construct(
        public int $line,
        public string $kind,
        public ?bool $zero,
        public bool $second,
    ) {
    }

    public function merge(self $other, string $context): self
    {
        if ($this->line !== $other->line || $this->kind !== $other->kind || ($this->zero === null) !== ($other->zero === null)) {
            throw new \RuntimeException("$context: the loop on line $this->line changed shape between two collections");
        }
        return new self($this->line, $this->kind, $this->zero === null ? null : ($this->zero || $other->zero), $this->second || $other->second);
    }

    /** @return array<string, mixed> */
    public function toArray(): array
    {
        return ['line' => $this->line, 'kind' => $this->kind, 'zero' => $this->zero, 'second' => $this->second];
    }

    public static function fromArray(mixed $data, string $source): self
    {
        if (
            !is_array($data)
            || !is_int($data['line'] ?? null)
            || !is_string($data['kind'] ?? null)
            || !(is_bool($data['zero'] ?? null) || ($data['zero'] ?? null) === null)
            || !is_bool($data['second'] ?? null)
        ) {
            throw new \RuntimeException("$source: malformed loop");
        }
        return new self($data['line'], $data['kind'], $data['zero'], $data['second']);
    }
}
