<?php

declare(strict_types=1);

namespace Opcov\Measure\Bench;

use Opcov\Measure\Json;

final readonly class Allocations
{
    private const array CALLS = ['malloc', 'calloc', 'realloc', 'aligned'];

    private function __construct(
        public int $calls,
        public int $frees,
        public int $bytes,
        public int $processes,
    ) {
    }

    public static function clear(string $prefix): void
    {
        foreach (glob("$prefix.*") ?: [] as $stale) {
            if (!unlink($stale)) {
                throw new \RuntimeException("cannot remove the stale allocation count $stale");
            }
        }
    }

    public static function collect(string $prefix): self
    {
        $files = glob("$prefix.*") ?: [];
        if ($files === []) {
            throw new \RuntimeException("the allocation counter wrote no $prefix.<pid>; did the process exit through exit()?");
        }
        $calls = 0;
        $frees = 0;
        $bytes = 0;
        foreach ($files as $file) {
            $counts = Json::read($file);
            foreach ([...self::CALLS, 'free', 'bytes'] as $key) {
                if (!is_int($counts[$key] ?? null)) {
                    throw new \RuntimeException("$file has no integer $key");
                }
            }
            $calls += array_sum(array_map(static fn (string $key): int => $counts[$key], self::CALLS));
            $frees += $counts['free'];
            $bytes += $counts['bytes'];
        }
        return new self($calls, $frees, $bytes, count($files));
    }

    /** @param array<mixed> $data */
    public static function fromArray(array $data): self
    {
        $counts = [];
        foreach (['calls', 'frees', 'bytes', 'processes'] as $key) {
            $counts[] = is_int($data[$key] ?? null) ? $data[$key] : throw new \RuntimeException("the allocations have no integer $key");
        }
        return new self(...$counts);
    }

    /** @return array{calls: int, frees: int, bytes: int, processes: int} */
    public function toArray(): array
    {
        return ['calls' => $this->calls, 'frees' => $this->frees, 'bytes' => $this->bytes, 'processes' => $this->processes];
    }
}
