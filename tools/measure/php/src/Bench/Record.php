<?php

declare(strict_types=1);

namespace Opcov\Measure\Bench;

use Opcov\Measure\ReportSize;

final readonly class Record
{
    /**
     * @param array<string, string> $build
     * @param array<string, string> $machine
     */
    public function __construct(
        public Key $key,
        public array $build,
        public string $measuredAt,
        public array $machine,
        public Result $result,
    ) {
        if (!$result->accepts($key->step)) {
            throw new \LogicException(sprintf('%s cannot hold a %s', $key->label(), $result::class));
        }
    }

    /** @param array<mixed> $data */
    public static function fromArray(array $data): self
    {
        return new self(
            Key::fromArray($data),
            self::strings($data['build'] ?? null, 'build'),
            is_string($data['measured_at'] ?? null) ? $data['measured_at'] : throw new \RuntimeException('no measured_at'),
            self::strings($data['machine'] ?? null, 'machine'),
            self::result($data),
        );
    }

    /** @return array<string, mixed> */
    public function toArray(): array
    {
        return $this->key->toArray() + ['build' => $this->build, 'measured_at' => $this->measuredAt, 'machine' => $this->machine] + $this->result->toArray();
    }

    /** @param array<mixed> $data */
    private static function result(array $data): Result
    {
        if (array_key_exists('timed_out_after_minutes', $data)) {
            return new TimedOut(is_int($data['timed_out_after_minutes']) ? $data['timed_out_after_minutes'] : throw new \RuntimeException('timed_out_after_minutes is not an integer'));
        }
        $tests = is_int($data['tests'] ?? null) ? $data['tests'] : throw new \RuntimeException('no integer tests');
        if (array_key_exists('sample', $data)) {
            return new Measured($tests, Sample::fromArray(is_array($data['sample']) ? $data['sample'] : throw new \RuntimeException('sample is not an object')));
        }
        return new PayloadSize(
            $tests,
            is_int($data['payload_bytes'] ?? null) ? $data['payload_bytes'] : throw new \RuntimeException('no integer payload_bytes'),
            ReportSize::fromArray($data['report'] ?? null, 'report'),
        );
    }

    /** @return array<string, string> */
    private static function strings(mixed $data, string $name): array
    {
        if (!is_array($data)) {
            throw new \RuntimeException("$name is not an object");
        }
        foreach ($data as $key => $value) {
            if (!is_string($key) || !is_string($value)) {
                throw new \RuntimeException("$name holds a non-string entry " . json_encode([$key => $value]));
            }
        }
        return $data;
    }
}
