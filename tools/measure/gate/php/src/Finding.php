<?php

declare(strict_types=1);

namespace Opcov\Measure\Gate;

final readonly class Finding
{
    public function __construct(
        public Category $category,
        public string $file,
        public string $function,
        public int $line,
        public string $detail,
    ) {
    }

    public function describe(): string
    {
        return sprintf('%s:%d %s (%s): %s', $this->file, $this->line, $this->function, $this->category->value, $this->detail);
    }
}
