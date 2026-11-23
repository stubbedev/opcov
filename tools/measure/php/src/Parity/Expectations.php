<?php

declare(strict_types=1);

namespace Opcov\Measure\Parity;

use Opcov\Measure\Coverage;
use Opcov\Measure\Driver;
use Opcov\Measure\Json;

final readonly class Expectations
{
    public const array OPCOV_ONLY_KINDS = [
        'CoalesceValue', 'CoalesceFallback',
        'TernaryValue', 'TernaryFallback',
        'NullsafeNull', 'NullsafeValue',
    ];

    /** @param array<string, list<bool>> $expected */
    private function __construct(
        private string $path,
        private array $expected,
    ) {
    }

    public static function load(string $path): self
    {
        $data = Json::read($path);
        $expected = [];
        foreach (['outcomes' => 'kind', 'loops' => 'flag'] as $section => $what) {
            $entries = $data[$section] ?? null;
            if (!is_array($entries) || !array_is_list($entries)) {
                throw new \RuntimeException("$path needs a '$section' list");
            }
            foreach ($entries as $index => $entry) {
                if (!is_array($entry) || !is_string($entry['function'] ?? null) || !is_int($entry['line'] ?? null) || !is_string($entry[$what] ?? null) || !is_bool($entry['hit'] ?? null)) {
                    throw new \RuntimeException("$path $section entry $index needs function, line, $what and hit");
                }
                if ($section === 'loops' && !in_array($entry['flag'], ['zero', 'second'], true)) {
                    throw new \RuntimeException("$path loops entry $index has flag {$entry['flag']}; expected zero or second");
                }
                if ($section === 'outcomes' && !in_array($entry['kind'], self::OPCOV_ONLY_KINDS, true)) {
                    throw new \RuntimeException("$path outcomes entry $index has kind {$entry['kind']}; only opcov's own splits (" . implode(', ', self::OPCOV_ONLY_KINDS) . ') need expectations, the rest is compared with Xdebug');
                }
                $expected[self::key($entry['function'], $entry['line'], $entry[$what])][] = $entry['hit'];
            }
        }
        return new self($path, $expected);
    }

    public function check(string $label, Coverage $opcov): Findings
    {
        $findings = new Findings();
        $actual = [];
        $files = [];
        foreach ($opcov->files() as $file) {
            foreach ($opcov->decisions($file) as $function => $decisions) {
                foreach ($decisions as $decision) {
                    foreach ($decision->outcomes as $outcome) {
                        if ($outcome->kind !== null && in_array($outcome->kind, self::OPCOV_ONLY_KINDS, true)) {
                            $key = self::key($function, $decision->line, $outcome->kind);
                            $actual[$key][] = $outcome->hit;
                            $files[$key] = $file;
                        }
                    }
                }
            }
            foreach ($opcov->loops($file) as $function => $loops) {
                foreach ($loops as $loop) {
                    foreach (['zero' => $loop->zero, 'second' => $loop->second] as $flag => $hit) {
                        if ($hit !== null) {
                            $key = self::key($function, $loop->line, $flag);
                            $actual[$key][] = $hit;
                            $files[$key] = $file;
                        }
                    }
                }
            }
        }
        $keys = array_keys($actual + $this->expected);
        sort($keys, SORT_STRING);
        foreach ($keys as $key) {
            $findings->expectationChecked();
            $have = $actual[$key] ?? [];
            $want = $this->expected[$key] ?? [];
            if ($have === $want) {
                continue;
            }
            [$function, $line, $what] = explode("\0", $key);
            $findings->diverge(new Divergence(
                Aspect::Expected,
                "$label/" . ($files[$key] ?? '?'),
                (int) $line,
                new Pair(Driver::Opcov, Driver::XdebugBranch),
                sprintf('%s %s: opcov %s, %s expects %s', $function, $what, self::words($have), $this->path, self::words($want)),
            ));
        }
        return $findings;
    }

    private static function key(string $function, int $line, string $what): string
    {
        return "$function\0$line\0$what";
    }

    /** @param list<?bool> $hits */
    private static function words(array $hits): string
    {
        if ($hits === []) {
            return 'nothing';
        }
        return '[' . implode(', ', array_map(static fn (?bool $hit): string => match ($hit) {
            true => 'hit',
            false => 'not hit',
            null => 'unknown',
        }, $hits)) . ']';
    }
}
