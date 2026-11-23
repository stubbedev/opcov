<?php

declare(strict_types=1);

namespace Opcov\Measure;

final readonly class Selection
{
    /**
     * @param list<string> $include
     * @param list<string> $exclude
     */
    public function __construct(
        public array $include,
        public array $exclude,
    ) {
        if ($include === []) {
            throw new \InvalidArgumentException('a selection needs at least one included path');
        }
    }

    public function contains(string $file): bool
    {
        return self::under($file, $this->include) && !self::under($file, $this->exclude);
    }

    public function excludePattern(): string
    {
        return '~^(?:' . implode('|', array_map(static fn (string $path): string => preg_quote(rtrim($path, '/') . '/', '~'), $this->exclude)) . ')~';
    }

    /** @return array<string, mixed> */
    public function toArray(): array
    {
        return ['include' => $this->include, 'exclude' => $this->exclude];
    }

    public static function fromArray(mixed $data, string $context): self
    {
        if (!is_array($data) || !self::isPathList($data['include'] ?? null) || !self::isPathList($data['exclude'] ?? null)) {
            throw new \RuntimeException("$context: a selection needs 'include' and 'exclude' lists of paths");
        }
        return new self($data['include'], $data['exclude']);
    }

    private static function isPathList(mixed $paths): bool
    {
        return is_array($paths) && array_is_list($paths) && array_filter($paths, 'is_string') === $paths;
    }

    /** @param list<string> $paths */
    private static function under(string $file, array $paths): bool
    {
        foreach ($paths as $path) {
            if (str_starts_with($file, rtrim($path, '/') . '/')) {
                return true;
            }
        }
        return false;
    }
}
