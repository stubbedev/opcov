<?php

declare(strict_types=1);

namespace Opcov\Measure;

final readonly class Outcome
{
    public function __construct(
        public int $target,
        public ?string $kind,
        public ?bool $hit,
    ) {
    }

    public function merge(self $other): self
    {
        if (($this->hit === null) !== ($other->hit === null)) {
            throw new \RuntimeException(sprintf('outcome to line %d is known to be hit in one collection and unknown in another', $this->target));
        }
        return new self($this->target, $this->kind, $this->hit === null ? null : ($this->hit || $other->hit));
    }

    /** @return array<string, mixed> */
    public function toArray(): array
    {
        return ['target' => $this->target, 'kind' => $this->kind, 'hit' => $this->hit];
    }

    public static function fromArray(mixed $data, string $source): self
    {
        if (
            !is_array($data)
            || !is_int($data['target'] ?? null)
            || !(is_string($data['kind'] ?? null) || ($data['kind'] ?? null) === null)
            || !(is_bool($data['hit'] ?? null) || ($data['hit'] ?? null) === null)
        ) {
            throw new \RuntimeException("$source: malformed outcome");
        }
        return new self($data['target'], $data['kind'], $data['hit']);
    }
}
