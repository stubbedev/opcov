<?php

declare(strict_types=1);

namespace Opcov\Measure\Session;

use Opcov\Measure\Coverage;
use Opcov\Measure\Decision;
use Opcov\Measure\Outcome;
use Opcov\Measure\Selection;

final class XdebugSession implements Session
{
    private const string ANONYMOUS_CLASS = 'class@anonymous';

    private const int EXIT = 2147483645;

    private readonly int $flags;

    /** @var array<string, array<string, array<int, array<string, mixed>>>> */
    private array $branchesSeen = [];

    public function __construct(private readonly Selection $selection, private readonly bool $branches)
    {
        xdebug_set_filter(XDEBUG_FILTER_CODE_COVERAGE, XDEBUG_PATH_INCLUDE, $selection->include);
        $this->flags = XDEBUG_CC_UNUSED | XDEBUG_CC_DEAD_CODE | ($branches ? XDEBUG_CC_BRANCH_CHECK : 0);
    }

    public function start(): void
    {
        xdebug_start_code_coverage($this->flags);
    }

    public function stop(string $test): mixed
    {
        $data = xdebug_get_code_coverage();
        xdebug_stop_code_coverage();
        return $data;
    }

    public function rearm(): void
    {
    }

    public function collect(string $test, Coverage $coverage): void
    {
        $this->record($this->stop($test), $coverage);
    }

    private function record(mixed $raw, Coverage $coverage): void
    {
        if (!is_array($raw)) {
            throw new \UnexpectedValueException('Xdebug returned ' . get_debug_type($raw) . ' instead of coverage');
        }
        $raw = array_filter($raw, fn (string|int $file): bool => $this->selection->contains((string) $file), ARRAY_FILTER_USE_KEY);
        if (!$this->branches) {
            foreach (LineHits::fromXdebugShape($raw, 'Xdebug') as $file => $lines) {
                $coverage->addLines($file, $lines);
            }
            return;
        }
        foreach ($raw as $file => $entry) {
            if (!is_array($entry) || !is_array($entry['lines'] ?? null) || !is_array($entry['functions'] ?? null)) {
                throw new \UnexpectedValueException("Xdebug branch coverage for $file lacks 'lines' or 'functions'");
            }
            $file = (string) $file;
            $coverage->addLines($file, LineHits::fromXdebugShape([$file => $entry['lines']], 'Xdebug')[$file]);
            foreach ($entry['functions'] as $name => $function) {
                $name = (string) $name;
                if (str_starts_with($name, self::ANONYMOUS_CLASS)) {
                    continue;
                }
                $branches = is_array($function) ? ($function['branches'] ?? null) : null;
                if (!is_array($branches)) {
                    throw new \UnexpectedValueException("Xdebug function $file $name has no branches");
                }
                $this->remember($file, $name, $branches);
            }
        }
    }

    /** @param array<int, array<string, mixed>> $branches */
    private function remember(string $file, string $name, array $branches): void
    {
        if (!isset($this->branchesSeen[$file][$name])) {
            $this->branchesSeen[$file][$name] = $branches;
            return;
        }
        foreach ($branches as $start => $branch) {
            foreach ($branch['out_hit'] ?? [] as $index => $hit) {
                if ($hit === 1) {
                    $this->branchesSeen[$file][$name][$start]['out_hit'][$index] = 1;
                }
            }
        }
    }

    public function writeReport(string $path, string $root): ?int
    {
        return null;
    }

    public function complete(Coverage $coverage): void
    {
        foreach ($this->branchesSeen as $file => $functions) {
            foreach ($functions as $name => $branches) {
                $coverage->addDecisions($file, self::key($name, $file), self::decisions($branches, "$file $name"));
            }
        }
    }

    private static function key(string $name, string $file): string
    {
        if ($name === '{main}') {
            return '{main}@' . $file;
        }
        if (preg_match('~^\{closure:(.+):(\d+)-(\d+)\}$~', $name, $match) === 1) {
            return sprintf('{closure}@%s:%s-%s', $match[1], $match[2], $match[3]);
        }
        return str_replace('->', '::', $name);
    }

    /**
     * @param array<int, array<string, mixed>> $branches
     * @return list<Decision>
     */
    private static function decisions(array $branches, string $context): array
    {
        ksort($branches);
        $decisions = [];
        foreach ($branches as $start => $branch) {
            $out = $branch['out'] ?? null;
            $outHit = $branch['out_hit'] ?? null;
            if (!is_array($out) || !is_array($outHit) || count($out) !== count($outHit) || !is_int($branch['line_end'] ?? null)) {
                throw new \UnexpectedValueException("Xdebug branch $start of $context is malformed");
            }
            $edges = [];
            foreach ($out as $index => $target) {
                if ($target !== self::EXIT) {
                    $edges[$target] = ($edges[$target] ?? false) || $outHit[$index] === 1;
                }
            }
            if (count($edges) < 2) {
                continue;
            }
            ksort($edges);
            $decisions[] = new Decision(
                $branch['line_end'],
                array_map(
                    static function (int $target, bool $hit) use ($branches, $context, $start): Outcome {
                        $line = $branches[$target]['line_start'] ?? throw new \UnexpectedValueException("Xdebug branch $start of $context leads to opcode $target, which starts no branch");
                        return new Outcome($line, null, $hit);
                    },
                    array_keys($edges),
                    array_values($edges),
                ),
            );
        }
        return $decisions;
    }
}
