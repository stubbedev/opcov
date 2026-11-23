<?php

declare(strict_types=1);

namespace Opcov\Measure\Gate;

final readonly class ChangedLines
{
    /**
     * @param array<string, list<int>> $lines
     * @param array<string, list<int>> $gapsBefore
     */
    private function __construct(
        public array $lines,
        public array $gapsBefore,
    ) {
    }

    public static function fromGitDiff(string $diff, string $source): self
    {
        $lines = [];
        $gaps = [];
        $file = null;
        foreach (explode("\n", $diff) as $number => $text) {
            if (str_starts_with($text, '+++ ')) {
                $path = substr($text, 4);
                $file = $path === '/dev/null' ? null : (str_starts_with($path, 'b/') ? substr($path, 2) : throw new \RuntimeException(sprintf('%s line %d: expected a b/ path, got %s', $source, $number + 1, $path)));
                continue;
            }
            if (!str_starts_with($text, '@@ ')) {
                continue;
            }
            if (preg_match('~^@@ -\d+(?:,(\d+))? \+(\d+)(?:,(\d+))? @@~', $text, $match) !== 1) {
                throw new \RuntimeException(sprintf('%s line %d: malformed hunk header %s', $source, $number + 1, $text));
            }
            if ($file === null) {
                continue;
            }
            $removed = $match[1] === '' ? 1 : (int) $match[1];
            $start = (int) $match[2];
            $count = isset($match[3]) && $match[3] !== '' ? (int) $match[3] : 1;
            if ($count === 0) {
                if ($removed > 0) {
                    $gaps[$file][] = $start + 1;
                }
                continue;
            }
            foreach (range($start, $start + $count - 1) as $line) {
                $lines[$file][] = $line;
            }
        }
        return new self($lines, $gaps);
    }

    /** @return list<string> */
    public function files(): array
    {
        $files = array_keys($this->lines + $this->gapsBefore);
        sort($files, SORT_STRING);
        return $files;
    }
}
