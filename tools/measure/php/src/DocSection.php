<?php

declare(strict_types=1);

namespace Opcov\Measure;

final class DocSection
{
    public static function replace(string $path, string $id, string $markdown): void
    {
        $contents = @file_get_contents($path);
        if ($contents === false) {
            throw new \RuntimeException("cannot read $path");
        }
        $begin = "<!-- $id:begin -->";
        $end = "<!-- $id:end -->";
        $start = strpos($contents, $begin);
        $stop = strpos($contents, $end);
        if ($start === false || $stop === false || $stop < $start) {
            throw new \RuntimeException("$path has no section delimited by $begin and $end");
        }
        $updated = substr($contents, 0, $start + strlen($begin)) . "\n" . rtrim($markdown) . "\n" . substr($contents, $stop);
        Filesystem::writeIfChanged($path, $updated);
    }

    /**
     * @param list<string> $header
     * @param list<list<string>> $rows
     */
    public static function table(array $header, array $rows): string
    {
        $line = static fn (array $cells): string => '| ' . implode(' | ', array_map(static fn (string $cell): string => str_replace('|', '\|', $cell), $cells)) . ' |';
        $lines = [$line($header), '|' . str_repeat('---|', count($header))];
        foreach ($rows as $row) {
            $lines[] = $line($row);
        }
        return implode("\n", $lines) . "\n";
    }
}
