<?php

declare(strict_types=1);

namespace Opcov\Measure;

final class Json
{
    public static function write(string $path, mixed $data): void
    {
        Filesystem::write($path, json_encode($data, JSON_THROW_ON_ERROR | JSON_PRETTY_PRINT | JSON_UNESCAPED_SLASHES | JSON_PRESERVE_ZERO_FRACTION) . "\n");
    }

    /** @return array<mixed> */
    public static function read(string $path): array
    {
        $contents = @file_get_contents($path);
        if ($contents === false) {
            throw new \RuntimeException("cannot read $path: " . (error_get_last()['message'] ?? 'unknown error'));
        }
        try {
            $data = json_decode($contents, true, flags: JSON_THROW_ON_ERROR);
        } catch (\JsonException $e) {
            throw new \RuntimeException("$path is not valid JSON: {$e->getMessage()}", 0, $e);
        }
        if (!is_array($data)) {
            throw new \RuntimeException("$path does not hold a JSON object or array");
        }
        return $data;
    }
}
