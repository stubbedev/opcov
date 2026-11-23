<?php

declare(strict_types=1);

namespace Opcov\Measure;

final class Filesystem
{
    public static function remove(string $path): void
    {
        if (is_link($path) || is_file($path)) {
            if (!unlink($path)) {
                throw new \RuntimeException("cannot remove $path");
            }
            return;
        }
        if (!is_dir($path)) {
            return;
        }
        $iterator = new \RecursiveIteratorIterator(
            new \RecursiveDirectoryIterator($path, \FilesystemIterator::SKIP_DOTS),
            \RecursiveIteratorIterator::CHILD_FIRST,
        );
        foreach ($iterator as $entry) {
            $name = $entry->getPathname();
            $removed = $entry->isDir() && !$entry->isLink() ? rmdir($name) : unlink($name);
            if (!$removed) {
                throw new \RuntimeException("cannot remove $name");
            }
        }
        if (!rmdir($path)) {
            throw new \RuntimeException("cannot remove $path");
        }
    }

    public static function writeIfChanged(string $path, string $contents): void
    {
        if (@file_get_contents($path) === $contents) {
            return;
        }
        self::write($path, $contents);
    }

    public static function write(string $path, string $contents): void
    {
        $directory = dirname($path);
        self::ensureDirectory($directory);
        $temporary = tempnam($directory, '.' . basename($path));
        if ($temporary === false) {
            throw new \RuntimeException("cannot create a temporary file next to $path");
        }
        if (file_put_contents($temporary, $contents) !== strlen($contents) || !rename($temporary, $path)) {
            unlink($temporary);
            throw new \RuntimeException("cannot write $path");
        }
    }

    public static function ensureDirectory(string $directory): void
    {
        if (!is_dir($directory) && !mkdir($directory, 0o777, true) && !is_dir($directory)) {
            throw new \RuntimeException("cannot create directory $directory");
        }
    }
}
