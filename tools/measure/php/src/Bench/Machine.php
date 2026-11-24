<?php

declare(strict_types=1);

namespace Opcov\Measure\Bench;

final readonly class Machine
{
    /** @return array<string, string> */
    public static function describe(): array
    {
        return [
            'cpu' => self::cpuModel(),
            'logical cpus' => (string) self::logicalCpus(),
            'governor' => self::read('/sys/devices/system/cpu/cpu0/cpufreq/scaling_governor') ?? 'unknown',
            'turbo' => self::turbo(),
            'kernel' => php_uname('s') . ' ' . php_uname('r'),
            'memory' => self::memory(),
            'perf_event_paranoid' => self::read('/proc/sys/kernel/perf_event_paranoid') ?? 'unknown',
            'load average at start' => implode(' ', array_map(static fn (float $load): string => sprintf('%.2f', $load), sys_getloadavg() ?: throw new \RuntimeException('cannot read the load average'))),
        ];
    }

    private static function cpuModel(): string
    {
        $info = self::read('/proc/cpuinfo') ?? throw new \RuntimeException('cannot read /proc/cpuinfo');
        if (preg_match('~^model name\s*:\s*(.+)$~m', $info, $match) !== 1) {
            throw new \RuntimeException('/proc/cpuinfo names no CPU model');
        }
        return trim($match[1]);
    }

    private static function logicalCpus(): int
    {
        $info = self::read('/proc/cpuinfo') ?? throw new \RuntimeException('cannot read /proc/cpuinfo');
        return preg_match_all('~^processor\s*:~m', $info);
    }

    private static function turbo(): string
    {
        $noTurbo = self::read('/sys/devices/system/cpu/intel_pstate/no_turbo');
        if ($noTurbo !== null) {
            return $noTurbo === '0' ? 'on (intel_pstate)' : 'off (intel_pstate)';
        }
        $boost = self::read('/sys/devices/system/cpu/cpufreq/boost');
        if ($boost !== null) {
            return $boost === '1' ? 'on (cpufreq boost)' : 'off (cpufreq boost)';
        }
        return 'unknown';
    }

    private static function memory(): string
    {
        $info = self::read('/proc/meminfo') ?? throw new \RuntimeException('cannot read /proc/meminfo');
        if (preg_match('~^MemTotal:\s*(\d+) kB$~m', $info, $match) !== 1) {
            throw new \RuntimeException('/proc/meminfo has no MemTotal');
        }
        return sprintf('%.1f GiB', (int) $match[1] / 1024 / 1024);
    }

    private static function read(string $path): ?string
    {
        $contents = @file_get_contents($path);
        return $contents === false ? null : trim($contents);
    }
}
