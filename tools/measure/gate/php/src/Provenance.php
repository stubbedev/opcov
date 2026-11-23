<?php

declare(strict_types=1);

namespace Opcov\Measure\Gate;

use Opcov\Measure\Process;
use Opcov\Measure\Toolchain;

final class Provenance
{
    public static function opcovCommit(Toolchain $toolchain): string
    {
        $commit = trim(self::git($toolchain, ['rev-parse', '--short=12', 'HEAD']));
        $changes = trim(self::git($toolchain, ['status', '--porcelain', '--untracked-files=no', '--', 'crates']));
        return $changes === '' ? $commit : "$commit with uncommitted changes to crates/";
    }

    /** @param list<string> $arguments */
    private static function git(Toolchain $toolchain, array $arguments): string
    {
        $command = [$toolchain->tool('git'), '-C', $toolchain->root, ...$arguments];
        $process = proc_open($command, [0 => ['file', '/dev/null', 'r'], 1 => ['pipe', 'w'], 2 => ['pipe', 'w']], $pipes, null, Process::cleanEnvironment());
        if ($process === false) {
            throw new \RuntimeException('cannot start ' . implode(' ', $command));
        }
        $output = stream_get_contents($pipes[1]);
        $errors = stream_get_contents($pipes[2]);
        fclose($pipes[1]);
        fclose($pipes[2]);
        $exit = proc_close($process);
        if ($exit !== 0 || $output === false) {
            throw new \RuntimeException(sprintf('%s exited with %d: %s', implode(' ', $command), $exit, $errors));
        }
        return $output;
    }
}
