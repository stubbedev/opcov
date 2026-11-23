<?php

declare(strict_types=1);

namespace Opcov\Measure\Gate;

use SebastianBergmann\CodeCoverage\CodeCoverage;
use SebastianBergmann\CodeCoverage\Data\ProcessedBranchCoverageData;
use SebastianBergmann\CodeCoverage\Data\ProcessedFunctionCoverageData;

final readonly class XdebugGate
{
    private const int EXIT = 2147483645;

    /**
     * @param array<string, array<int, list<string>|null>> $lineCoverage
     * @param array<string, array<string, ProcessedFunctionCoverageData>> $functionCoverage
     */
    private function __construct(
        private string $root,
        private array $lineCoverage,
        private array $functionCoverage,
    ) {
    }

    public static function load(string $root, string $cov): self
    {
        $coverage = require $cov;
        if (!$coverage instanceof CodeCoverage) {
            throw new \RuntimeException("$cov holds " . get_debug_type($coverage) . ', not a php-code-coverage CodeCoverage');
        }
        $data = $coverage->getData();
        return new self(rtrim($root, '/'), $data->lineCoverage(), $data->functionCoverage());
    }

    /** @return array<string, mixed> */
    public function evaluate(ChangedLines $changes): array
    {
        $files = [];
        $unmeasured = [];
        foreach ($changes->files() as $relative) {
            $absolute = "$this->root/$relative";
            if (!isset($this->lineCoverage[$absolute])) {
                $unmeasured[] = $relative;
                continue;
            }
            $map = FunctionMap::parse($absolute, $relative);
            $names = $this->xdebugNames($absolute, $relative);
            $functions = [];
            foreach ($map->select($changes->lines[$relative] ?? [], $changes->gapsBefore[$relative] ?? []) as $function) {
                $functions[$function->key] = $this->function($map, $function, $absolute, $names[$function->key] ?? null);
            }
            $files[$relative] = $functions;
        }
        return ['files' => $files, 'unmeasured' => $unmeasured];
    }

    /** @return array<string, ProcessedFunctionCoverageData> */
    private function xdebugNames(string $absolute, string $relative): array
    {
        $names = [];
        foreach ($this->functionCoverage[$absolute] ?? [] as $name => $data) {
            $names[$this->key((string) $name, $absolute, $relative)] = $data;
        }
        return $names;
    }

    private function key(string $name, string $absolute, string $relative): string
    {
        if ($name === '{main}') {
            return "{main}@$relative";
        }
        $closure = '{closure:' . $absolute . ':';
        if (str_starts_with($name, $closure) && preg_match('~^(\d+)-(\d+)\}$~', substr($name, strlen($closure)), $match) === 1) {
            return "{closure}@$relative:{$match[1]}-{$match[2]}";
        }
        return str_replace('->', '::', $name);
    }

    /** @return array<string, mixed> */
    private function function(FunctionMap $map, FunctionSpan $function, string $absolute, ?ProcessedFunctionCoverageData $data): array
    {
        $executable = [];
        $uncovered = [];
        foreach ($map->ownedLines($function, $function->start, $function->end) as $line) {
            $tests = $this->lineCoverage[$absolute][$line] ?? null;
            if ($tests === null) {
                continue;
            }
            $executable[] = $line;
            if ($tests === []) {
                $uncovered[] = $line;
            }
        }
        $result = [
            'span' => [$function->start, $function->end],
            'xdebug_function' => $data !== null,
            'executable_lines' => $executable,
            'uncovered_lines' => $uncovered,
            'unexecuted_branches' => [],
            'decisions' => [],
            'paths' => 0,
            'paths_never_taken' => 0,
        ];
        if ($data === null) {
            return $result;
        }
        $branches = $data->branches;
        ksort($branches);
        $predecessors = [];
        foreach ($branches as $start => $branch) {
            foreach (self::targets($branch) as $target) {
                $predecessors[$target][] = $start;
            }
        }
        foreach ($branches as $start => $branch) {
            $targets = self::targets($branch);
            if ($branch->hit !== [] && count($targets) >= 2) {
                $result['decisions'][] = [
                    'line' => $branch->line_end,
                    'all_successors_ran' => array_all($targets, static fn (int $target): bool => ($branches[$target]->hit ?? []) !== []),
                ];
            }
            if ($branch->hit !== []) {
                continue;
            }
            $result['unexecuted_branches'][] = [
                'op_start' => $start,
                'line_start' => $branch->line_start,
                'line_end' => $branch->line_end,
                'entries' => array_map(
                    static fn (int $from): array => [
                        'line' => $branches[$from]->line_end,
                        'ran' => $branches[$from]->hit !== [],
                        'decision' => count(self::targets($branches[$from])) >= 2,
                    ],
                    $predecessors[$start] ?? [],
                ),
            ];
        }
        $result['paths'] = count($data->paths);
        $result['paths_never_taken'] = count(array_filter($data->paths, static fn (object $path): bool => $path->hit === []));
        return $result;
    }

    /** @return list<int> */
    private static function targets(ProcessedBranchCoverageData $branch): array
    {
        $targets = array_values(array_unique(array_filter($branch->out, static fn (int $target): bool => $target !== self::EXIT)));
        sort($targets);
        return $targets;
    }
}
