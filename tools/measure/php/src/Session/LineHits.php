<?php

declare(strict_types=1);

namespace Opcov\Measure\Session;

final class LineHits
{
    /** @return array<string, array<int, bool>> */
    public static function fromXdebugShape(mixed $raw, string $driver): array
    {
        if (!is_array($raw)) {
            throw new \UnexpectedValueException("$driver returned " . get_debug_type($raw) . ' instead of file => line => status');
        }
        $files = [];
        foreach ($raw as $file => $lines) {
            if (!is_array($lines)) {
                throw new \UnexpectedValueException("$driver returned a non-array line map for $file");
            }
            $files[$file] = self::statuses($lines, $driver, (string) $file);
        }
        return $files;
    }

    /**
     * @param array<mixed> $lines
     * @return array<int, bool>
     */
    private static function statuses(array $lines, string $driver, string $file): array
    {
        $statuses = [];
        foreach ($lines as $line => $status) {
            $hit = match ($status) {
                1 => true,
                -1 => false,
                -2 => null,
                default => throw new \UnexpectedValueException("$driver reported status " . var_export($status, true) . " for $file:$line"),
            };
            if ($hit !== null) {
                $statuses[(int) $line] = $hit;
            }
        }
        return $statuses;
    }
}
