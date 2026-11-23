<?php

declare(strict_types=1);

namespace Opcov\Measure\Gate;

use Opcov\Measure\Json;

final readonly class Explanations
{
    /** @param list<array{category: Category, file: string, function: string, lines: list<int>, reason: string}> $entries */
    private function __construct(private array $entries)
    {
    }

    public static function load(string $path): self
    {
        $entries = [];
        foreach (Json::read($path) as $index => $entry) {
            $context = "$path entry $index";
            if (!is_array($entry) || !is_string($entry['category'] ?? null) || !is_string($entry['file'] ?? null) || !is_string($entry['function'] ?? null) || !is_string($entry['reason'] ?? null) || $entry['reason'] === '') {
                throw new \RuntimeException("$context needs 'category', 'file', 'function' and a non-empty 'reason' as strings");
            }
            $lines = $entry['lines'] ?? null;
            if (!is_array($lines) || $lines === [] || !array_is_list($lines) || array_filter($lines, 'is_int') !== $lines) {
                throw new \RuntimeException("$context needs 'lines' as a non-empty list of integers");
            }
            $category = Category::tryFrom($entry['category']) ?? throw new \RuntimeException("$context names unknown category {$entry['category']}");
            if ($category->explainedByRule()) {
                throw new \RuntimeException("$context explains category {$category->value}, which needs no entry");
            }
            $entries[] = ['category' => $category, 'file' => $entry['file'], 'function' => $entry['function'], 'lines' => $lines, 'reason' => $entry['reason']];
        }
        return new self($entries);
    }

    /**
     * @param list<Finding> $findings
     * @return array{explained: list<array{finding: Finding, reason: string}>, unexplained: list<Finding>, stale: list<string>}
     */
    public function judge(array $findings): array
    {
        $explained = [];
        $unexplained = [];
        $used = [];
        foreach ($findings as $finding) {
            if ($finding->category->explainedByRule()) {
                continue;
            }
            $match = null;
            foreach ($this->entries as $index => $entry) {
                if ($entry['category'] === $finding->category && $entry['file'] === $finding->file && $entry['function'] === $finding->function && in_array($finding->line, $entry['lines'], true)) {
                    $match = $index;
                    break;
                }
            }
            if ($match === null) {
                $unexplained[] = $finding;
                continue;
            }
            $used[$match][$finding->line] = true;
            $explained[] = ['finding' => $finding, 'reason' => $this->entries[$match]['reason']];
        }
        $stale = [];
        foreach ($this->entries as $index => $entry) {
            foreach ($entry['lines'] as $line) {
                if (!isset($used[$index][$line])) {
                    $stale[] = sprintf('%s:%d %s (%s)', $entry['file'], $line, $entry['function'], $entry['category']->value);
                }
            }
        }
        return ['explained' => $explained, 'unexplained' => $unexplained, 'stale' => $stale];
    }
}
