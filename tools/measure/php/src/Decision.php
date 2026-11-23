<?php

declare(strict_types=1);

namespace Opcov\Measure;

final readonly class Decision
{
    /** @param list<Outcome> $outcomes */
    public function __construct(
        public int $line,
        public array $outcomes,
    ) {
    }

    /** @return list<int> */
    public function targets(): array
    {
        return array_map(static fn (Outcome $outcome): int => $outcome->target, $this->outcomes);
    }

    public function sameShape(self $other): bool
    {
        return $this->line === $other->line && $this->targets() === $other->targets();
    }

    public function merge(self $other, string $context): self
    {
        if (!$this->sameShape($other)) {
            throw new \RuntimeException(sprintf('%s: the decision on line %d changed shape between two collections', $context, $this->line));
        }
        return new self(
            $this->line,
            array_map(static fn (Outcome $left, Outcome $right): Outcome => $left->merge($right), $this->outcomes, $other->outcomes),
        );
    }

    /** @return array<string, mixed> */
    public function toArray(): array
    {
        return [
            'line' => $this->line,
            'outcomes' => array_map(static fn (Outcome $outcome): array => $outcome->toArray(), $this->outcomes),
        ];
    }

    public static function fromArray(mixed $data, string $source): self
    {
        if (!is_array($data) || !is_int($data['line'] ?? null) || !is_array($data['outcomes'] ?? null)) {
            throw new \RuntimeException("$source: malformed decision");
        }
        return new self(
            $data['line'],
            array_map(static fn (mixed $outcome): Outcome => Outcome::fromArray($outcome, $source), array_values($data['outcomes'])),
        );
    }
}
