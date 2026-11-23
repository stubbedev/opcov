<?php

declare(strict_types=1);

namespace Opcov\Measure;

final readonly class Config
{
    public const string VARIABLE = 'OPCOV_MEASURE';

    public function __construct(
        public Driver $driver,
        public Mode $mode,
        public Selection $selection,
        public string $root,
        public string $output,
    ) {
    }

    /** @return array<string, string> */
    public function environment(): array
    {
        return [self::VARIABLE => json_encode([
            'driver' => $this->driver->value,
            'mode' => $this->mode->value,
            'selection' => $this->selection->toArray(),
            'root' => $this->root,
            'output' => $this->output,
        ], JSON_THROW_ON_ERROR | JSON_UNESCAPED_SLASHES)];
    }

    public static function fromEnvironment(): self
    {
        $raw = getenv(self::VARIABLE);
        if ($raw === false) {
            throw new \RuntimeException(self::VARIABLE . ' is not set; this script runs under tools/measure');
        }
        $data = json_decode($raw, true, flags: JSON_THROW_ON_ERROR);
        if (
            !is_array($data)
            || !is_string($data['driver'] ?? null)
            || !is_string($data['mode'] ?? null)
            || !is_string($data['root'] ?? null)
            || !is_string($data['output'] ?? null)
        ) {
            throw new \RuntimeException(self::VARIABLE . ' is malformed: ' . $raw);
        }
        return new self(
            Driver::parse($data['driver']),
            Mode::tryFrom($data['mode']) ?? throw new \RuntimeException(self::VARIABLE . ' names unknown mode ' . $data['mode']),
            Selection::fromArray($data['selection'] ?? null, self::VARIABLE),
            $data['root'],
            $data['output'],
        );
    }
}
