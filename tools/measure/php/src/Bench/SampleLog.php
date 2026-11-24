<?php

declare(strict_types=1);

namespace Opcov\Measure\Bench;

use Opcov\Measure\Filesystem;
use Opcov\Measure\Json;

final class SampleLog
{
    /**
     * @param list<Record> $records
     * @param array<string, Record> $latest
     */
    private function __construct(
        public readonly string $path,
        private array $records,
        private array $latest,
    ) {
    }

    public static function open(string $path): self
    {
        $contents = @file_get_contents($path);
        if ($contents === false) {
            if (is_file($path)) {
                throw new \RuntimeException("cannot read the benchmark samples $path: " . (error_get_last()['message'] ?? 'unknown error'));
            }
            return new self($path, [], []);
        }
        $lines = explode("\n", $contents);
        $partial = array_pop($lines);
        $records = [];
        foreach ($lines as $index => $line) {
            try {
                $records[] = Record::fromArray(Json::decode($line));
            } catch (\RuntimeException|\InvalidArgumentException $e) {
                throw new \RuntimeException(sprintf('%s:%d holds no benchmark sample', $path, $index + 1), 0, $e);
            }
        }
        $log = new self($path, [], []);
        foreach ($records as $record) {
            $log->remember($record);
        }
        if ($partial !== '') {
            fwrite(STDERR, sprintf("bench: dropping the incomplete last line of %s, a sample whose write was interrupted\n", $path));
            $log->rewrite();
        }
        return $log;
    }

    /** @param array<string, string> $build */
    public function find(Key $key, array $build): ?Record
    {
        return $this->latest[self::index($key, $build)] ?? null;
    }

    public function append(Record $record): void
    {
        Filesystem::ensureDirectory(dirname($this->path));
        $line = Json::line($record->toArray());
        if (file_put_contents($this->path, $line, FILE_APPEND | LOCK_EX) !== strlen($line)) {
            throw new \RuntimeException("cannot append {$record->key->label()} to $this->path");
        }
        $this->remember($record);
    }

    /** @param \Closure(Record): bool $discarded */
    public function discard(\Closure $discarded): int
    {
        $kept = array_values(array_filter($this->records, static fn (Record $record): bool => !$discarded($record)));
        $count = count($this->records) - count($kept);
        $this->records = [];
        $this->latest = [];
        foreach ($kept as $record) {
            $this->remember($record);
        }
        $this->rewrite();
        return $count;
    }

    private function remember(Record $record): void
    {
        $this->records[] = $record;
        $this->latest[self::index($record->key, $record->build)] = $record;
    }

    private function rewrite(): void
    {
        Filesystem::write($this->path, implode('', array_map(static fn (Record $record): string => Json::line($record->toArray()), $this->records)));
    }

    /** @param array<string, string> $build */
    private static function index(Key $key, array $build): string
    {
        ksort($build);
        return Json::line([$key->toArray(), $build]);
    }
}
