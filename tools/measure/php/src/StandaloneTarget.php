<?php

declare(strict_types=1);

namespace Opcov\Measure;

final readonly class StandaloneTarget implements Target
{
    public function __construct(
        private string $label,
        private Workload $workload,
        private ?string $expectations,
    ) {
    }

    public function xdebug(): Driver
    {
        return Driver::XdebugBranch;
    }

    public function expectations(): ?string
    {
        return $this->expectations;
    }

    public function label(): string
    {
        return $this->label;
    }

    public function root(): string
    {
        return $this->workload->sources;
    }

    public function selection(): Selection
    {
        return new Selection([$this->workload->sources], []);
    }

    public function environment(): array
    {
        return [];
    }

    public function cwd(): string
    {
        return dirname($this->workload->runner);
    }

    public function script(): array
    {
        return [dirname(__DIR__) . '/collect.php', $this->workload->sources, $this->workload->runner];
    }

    public function acceptedExits(): array
    {
        return [0];
    }
}
