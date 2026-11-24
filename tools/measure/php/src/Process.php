<?php

declare(strict_types=1);

namespace Opcov\Measure;

final readonly class Process
{
    public const int TIMED_OUT = 124;

    private const array INHERITED = ['PATH', 'HOME', 'TMPDIR', 'LANG', 'LC_ALL', 'TZ', 'SSL_CERT_FILE', 'NIX_SSL_CERT_FILE'];

    /**
     * @param list<string> $command
     * @param array<string, string> $environment
     * @param list<string> $monitor a command prefix that runs GNU time and the command under it (perf stat)
     */
    public static function run(Toolchain $toolchain, array $command, array $environment, string $cwd, string $log, int $budgetSeconds, array $monitor = []): ProcessResult
    {
        $rss = "$log.time";
        $wrapped = [
            $toolchain->tool('timeout'), '--kill-after=10', (string) $budgetSeconds,
            ...$monitor,
            $toolchain->tool('time'), '--verbose', '--output=' . $rss,
            ...$command,
        ];
        Filesystem::ensureDirectory(dirname($log));
        $started = hrtime(true);
        $process = proc_open(
            $wrapped,
            [0 => ['file', '/dev/null', 'r'], 1 => ['file', $log, 'w'], 2 => ['redirect', 1]],
            $pipes,
            $cwd,
            $environment + self::cleanEnvironment(),
        );
        if ($process === false) {
            throw new \RuntimeException('cannot start ' . implode(' ', $command));
        }
        $exit = proc_close($process);
        $seconds = (hrtime(true) - $started) / 1e9;
        return new ProcessResult($exit, $seconds, self::peakRss($rss, $exit), $log);
    }

    /** @return array<string, string> */
    public static function cleanEnvironment(): array
    {
        $environment = [];
        foreach (self::INHERITED as $name) {
            $value = getenv($name);
            if ($value !== false) {
                $environment[$name] = $value;
            }
        }
        return $environment;
    }

    private static function peakRss(string $file, int $exit): ?int
    {
        $report = @file_get_contents($file);
        if ($report === false) {
            if ($exit === self::TIMED_OUT) {
                return null;
            }
            throw new \RuntimeException("GNU time wrote no report to $file");
        }
        if (preg_match('~Maximum resident set size \(kbytes\): (\d+)~', $report, $match) !== 1) {
            if ($exit === self::TIMED_OUT) {
                return null;
            }
            throw new \RuntimeException("GNU time's report $file has no maximum resident set size");
        }
        return (int) $match[1] * 1024;
    }
}
