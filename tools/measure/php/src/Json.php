<?php

declare(strict_types=1);

namespace Opcov\Measure;

final class Json
{
    private const int FLAGS = JSON_THROW_ON_ERROR | JSON_UNESCAPED_SLASHES | JSON_PRESERVE_ZERO_FRACTION;

    public static function write(string $path, mixed $data): void
    {
        Filesystem::write($path, json_encode($data, self::FLAGS | JSON_PRETTY_PRINT) . "\n");
    }

    public static function line(mixed $data): string
    {
        return json_encode($data, self::FLAGS) . "\n";
    }

    /** @return array<mixed> */
    public static function read(string $path): array
    {
        $contents = @file_get_contents($path);
        if ($contents === false) {
            throw new \RuntimeException("cannot read $path: " . (error_get_last()['message'] ?? 'unknown error'));
        }
        try {
            return self::decode($contents);
        } catch (\RuntimeException $e) {
            throw new \RuntimeException("cannot parse $path", 0, $e);
        }
    }

    /** @return array<mixed> */
    public static function decode(string $contents): array
    {
        try {
            $data = json_decode($contents, true, flags: JSON_THROW_ON_ERROR);
        } catch (\JsonException $e) {
            throw new \RuntimeException('not valid JSON', 0, $e);
        }
        if (!is_array($data)) {
            throw new \RuntimeException('does not hold a JSON object or array');
        }
        return $data;
    }
}
