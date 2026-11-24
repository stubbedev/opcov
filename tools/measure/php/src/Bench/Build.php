<?php

declare(strict_types=1);

namespace Opcov\Measure\Bench;

use Opcov\Measure\Driver;
use Opcov\Measure\Toolchain;

final readonly class Build
{
    private function __construct(
        private string $phpRelease,
        private string $environment,
        private string $opcov,
    ) {
    }

    public static function of(Toolchain $toolchain): self
    {
        $hash = hash_file('sha256', $toolchain->opcov);
        if ($hash === false) {
            throw new \RuntimeException("cannot hash the opcov extension $toolchain->opcov");
        }
        return new self($toolchain->phpRelease, $toolchain->environment, "sha256:$hash");
    }

    /** @return array<string, string> */
    public function identity(Driver $driver): array
    {
        $identity = ['php' => $this->phpRelease, 'environment' => $this->environment];
        return $driver === Driver::Opcov ? $identity + ['opcov' => $this->opcov] : $identity;
    }
}
