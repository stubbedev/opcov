<?php

declare(strict_types=1);

namespace Opcov\Measure;

interface Target
{
    public function label(): string;

    public function root(): string;

    public function selection(): Selection;

    /** @return array<string, string> */
    public function environment(): array;

    public function cwd(): string;

    /** @return list<string> */
    public function script(): array;

    /** @return list<int> */
    public function acceptedExits(): array;

    public function expectations(): ?string;

    public function xdebug(): Driver;
}
