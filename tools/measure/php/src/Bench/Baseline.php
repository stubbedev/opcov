<?php

declare(strict_types=1);

namespace Opcov\Measure\Bench;

use Opcov\Measure\Driver;
use Opcov\Measure\Json;

final readonly class Baseline
{
    public function __construct(private string $path)
    {
    }

    public function store(string $php, BenchReport $report): void
    {
        $data = is_file($this->path) ? Json::read($this->path) : [];
        $ratios = $data[$php]['opcov_over_none'] ?? [];
        foreach ($report->workloads as $name => $result) {
            $ratios[$name] = round($result->overhead(Driver::Opcov) ?? throw new \LogicException("$name ran without the no-coverage baseline"), 4);
        }
        ksort($ratios);
        $data[$php] = ['machine' => $report->toArray()['machine'], 'opcov_over_none' => $ratios];
        ksort($data);
        Json::write($this->path, $data);
    }

    /** @return list<string> */
    public function regressions(string $php, BenchReport $report, float $tolerance): array
    {
        $ratios = Json::read($this->path)[$php]['opcov_over_none'] ?? throw new \RuntimeException("$this->path has no baseline for PHP $php; record one with --update-baseline");
        $regressions = [];
        foreach ($report->workloads as $name => $result) {
            $baseline = $ratios[$name] ?? throw new \RuntimeException("$this->path has no PHP $php baseline for workload $name");
            $current = $result->overhead(Driver::Opcov) ?? throw new \LogicException("$name ran without the no-coverage baseline");
            if ($current > $baseline * $tolerance) {
                $regressions[] = sprintf('%s: opcov takes %.3fx the time of no coverage, the baseline is %.3fx (tolerance %.0f%%)', $name, $current, $baseline, ($tolerance - 1) * 100);
            }
        }
        return $regressions;
    }
}
