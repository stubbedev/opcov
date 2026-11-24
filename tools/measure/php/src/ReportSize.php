<?php

declare(strict_types=1);

namespace Opcov\Measure;

final readonly class ReportSize
{
    private function __construct(
        public ?int $bytes,
        public ?string $refusal,
    ) {
    }

    public static function none(): self
    {
        return new self(null, null);
    }

    public static function written(int $bytes): self
    {
        return new self($bytes, null);
    }

    public static function refused(string $reason): self
    {
        return new self(null, $reason);
    }

    /** @return array{bytes: int}|array{refused: string}|null */
    public function toArray(): ?array
    {
        if ($this->bytes !== null) {
            return ['bytes' => $this->bytes];
        }
        if ($this->refusal !== null) {
            return ['refused' => $this->refusal];
        }
        return null;
    }

    public static function fromArray(mixed $data, string $context): self
    {
        if ($data === null) {
            return self::none();
        }
        if (is_array($data) && count($data) === 1 && is_int($data['bytes'] ?? null)) {
            return self::written($data['bytes']);
        }
        if (is_array($data) && count($data) === 1 && is_string($data['refused'] ?? null)) {
            return self::refused($data['refused']);
        }
        throw new \RuntimeException("$context: malformed report size " . json_encode($data));
    }
}
