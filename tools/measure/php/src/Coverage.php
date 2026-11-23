<?php

declare(strict_types=1);

namespace Opcov\Measure;

final class Coverage
{
    /** @var array<string, array<int, bool>> */
    private array $lines = [];

    /** @var array<string, array<string, list<Decision>>> */
    private array $decisions = [];

    /** @var array<string, array<string, list<Loop>>> */
    private array $loops = [];

    /** @param array<int, bool> $lines */
    public function addLines(string $file, array $lines): void
    {
        $known = $this->lines[$file] ?? [];
        foreach ($lines as $line => $hit) {
            $known[$line] = ($known[$line] ?? false) || $hit;
        }
        $this->lines[$file] = $known;
    }

    /** @param list<Decision> $decisions */
    public function addDecisions(string $file, string $function, array $decisions): void
    {
        $known = $this->decisions[$file][$function] ?? null;
        if ($known === null) {
            $this->decisions[$file][$function] = $decisions;
            return;
        }
        if (count($known) !== count($decisions)) {
            throw new \RuntimeException(sprintf('%s %s: the driver reported %d decisions before and %d now', $file, $function, count($known), count($decisions)));
        }
        $this->decisions[$file][$function] = array_map(
            static fn (Decision $left, Decision $right): Decision => $left->merge($right, "$file $function"),
            $known,
            $decisions,
        );
    }

    /** @param list<Loop> $loops */
    public function addLoops(string $file, string $function, array $loops): void
    {
        $known = $this->loops[$file][$function] ?? null;
        if ($known === null) {
            $this->loops[$file][$function] = $loops;
            return;
        }
        if (count($known) !== count($loops)) {
            throw new \RuntimeException(sprintf('%s %s: the driver reported %d loops before and %d now', $file, $function, count($known), count($loops)));
        }
        $this->loops[$file][$function] = array_map(
            static fn (Loop $left, Loop $right): Loop => $left->merge($right, "$file $function"),
            $known,
            $loops,
        );
    }

    /** @return array<string, list<Loop>> */
    public function loops(string $file): array
    {
        return $this->loops[$file] ?? [];
    }

    /** @return list<string> */
    public function files(): array
    {
        $files = array_keys($this->lines + $this->decisions + $this->loops);
        sort($files, SORT_STRING);
        return $files;
    }

    /** @return array<int, bool> */
    public function lines(string $file): array
    {
        return $this->lines[$file] ?? [];
    }

    /** @return array<string, list<Decision>> */
    public function decisions(string $file): array
    {
        return $this->decisions[$file] ?? [];
    }

    public function relativeTo(string $root): self
    {
        $prefix = rtrim($root, '/') . '/';
        $relative = static function (string $path) use ($prefix): string {
            return str_replace($prefix, '', $path);
        };
        $result = new self();
        foreach ($this->lines as $file => $lines) {
            if (!str_starts_with($file, $prefix)) {
                throw new \RuntimeException(sprintf('covered file %s is outside the measured root %s', $file, $prefix));
            }
            $result->addLines($relative($file), $lines);
        }
        foreach ($this->decisions as $file => $functions) {
            foreach ($functions as $function => $decisions) {
                $result->addDecisions($relative($file), $relative($function), $decisions);
            }
        }
        foreach ($this->loops as $file => $functions) {
            foreach ($functions as $function => $loops) {
                $result->addLoops($relative($file), $relative($function), $loops);
            }
        }
        return $result;
    }

    /** @return array<string, mixed> */
    public function toArray(): array
    {
        $files = [];
        foreach ($this->files() as $file) {
            $lines = $this->lines($file);
            ksort($lines);
            $functions = $this->decisions($file);
            ksort($functions, SORT_STRING);
            $loops = $this->loops($file);
            ksort($loops, SORT_STRING);
            $files[$file] = [
                'lines' => (object) $lines,
                'functions' => (object) array_map(
                    static fn (array $decisions): array => array_map(static fn (Decision $decision): array => $decision->toArray(), $decisions),
                    $functions,
                ),
                'loops' => (object) array_map(
                    static fn (array $loops): array => array_map(static fn (Loop $loop): array => $loop->toArray(), $loops),
                    $loops,
                ),
            ];
        }
        return ['files' => (object) $files];
    }

    /** @param array<string, mixed> $data */
    public static function fromArray(array $data, string $source): self
    {
        $coverage = new self();
        $files = $data['files'] ?? throw new \RuntimeException("$source: missing 'files'");
        if (!is_array($files)) {
            throw new \RuntimeException("$source: 'files' is not an object");
        }
        foreach ($files as $file => $entry) {
            if (!is_array($entry) || !is_array($entry['lines'] ?? null) || !is_array($entry['functions'] ?? null) || !is_array($entry['loops'] ?? null)) {
                throw new \RuntimeException("$source: file $file lacks 'lines', 'functions' or 'loops'");
            }
            $lines = [];
            foreach ($entry['lines'] as $line => $hit) {
                if (!is_int($line) || !is_bool($hit)) {
                    throw new \RuntimeException("$source: file $file has a malformed line entry");
                }
                $lines[$line] = $hit;
            }
            $coverage->addLines((string) $file, $lines);
            foreach ($entry['functions'] as $function => $decisions) {
                if (!is_array($decisions)) {
                    throw new \RuntimeException("$source: file $file function $function is not a list");
                }
                $coverage->addDecisions(
                    (string) $file,
                    (string) $function,
                    array_map(static fn (mixed $decision): Decision => Decision::fromArray($decision, "$source $file $function"), array_values($decisions)),
                );
            }
            foreach ($entry['loops'] as $function => $loops) {
                if (!is_array($loops)) {
                    throw new \RuntimeException("$source: file $file loops of $function are not a list");
                }
                $coverage->addLoops(
                    (string) $file,
                    (string) $function,
                    array_map(static fn (mixed $loop): Loop => Loop::fromArray($loop, "$source $file $function"), array_values($loops)),
                );
            }
        }
        return $coverage;
    }
}
