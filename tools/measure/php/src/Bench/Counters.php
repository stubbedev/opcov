<?php

declare(strict_types=1);

namespace Opcov\Measure\Bench;

final readonly class Counters
{
    public const array EVENTS = ['instructions', 'cycles', 'branches', 'branch-misses', 'cache-misses', 'page-faults', 'task-clock'];

    /**
     * @param array<string, ?float> $values the count per event, null when perf could not count it
     * @param array<string, string> $reported the event name as perf reported it, with its modifiers (cycles:u counts user space only)
     */
    private function __construct(
        public array $values,
        public array $reported,
    ) {
    }

    /** @return list<string> */
    public static function perfArguments(string $output): array
    {
        return ['stat', '-x', ',', '-o', $output, '-e', implode(',', self::EVENTS), '--'];
    }

    public static function read(string $path): self
    {
        $lines = @file($path, FILE_IGNORE_NEW_LINES | FILE_SKIP_EMPTY_LINES);
        if ($lines === false) {
            throw new \RuntimeException("perf stat wrote no counters to $path");
        }
        $values = [];
        $reported = [];
        foreach ($lines as $line) {
            if (str_starts_with($line, '#')) {
                continue;
            }
            $fields = str_getcsv($line, ',', '"', '');
            $value = $fields[0] ?? '';
            $name = $fields[2] ?? '';
            $event = explode(':', $name, 2)[0];
            if (!in_array($event, self::EVENTS, true)) {
                throw new \RuntimeException("$path has a line for an event that was not requested: $line");
            }
            $reported[$event] = $name;
            $values[$event] = is_numeric($value) ? (float) $value : null;
        }
        foreach (self::EVENTS as $event) {
            if (!array_key_exists($event, $values)) {
                throw new \RuntimeException("$path has no line for $event");
            }
        }
        return new self($values, $reported);
    }

    /** @param array<mixed> $data */
    public static function fromArray(array $data): self
    {
        $counted = is_array($data['values'] ?? null) ? $data['values'] : throw new \RuntimeException('the counters have no values');
        $names = is_array($data['reported'] ?? null) ? $data['reported'] : throw new \RuntimeException('the counters have no reported names');
        $values = [];
        $reported = [];
        foreach (self::EVENTS as $event) {
            if (!array_key_exists($event, $counted) || !is_string($names[$event] ?? null)) {
                throw new \RuntimeException("the counters have no $event");
            }
            $value = $counted[$event];
            $values[$event] = match (true) {
                $value === null => null,
                is_int($value), is_float($value) => (float) $value,
                default => throw new \RuntimeException("the $event counter is not a number"),
            };
            $reported[$event] = $names[$event];
        }
        return new self($values, $reported);
    }

    /** @return array{values: array<string, ?float>, reported: array<string, string>} */
    public function toArray(): array
    {
        return ['values' => $this->values, 'reported' => $this->reported];
    }
}
