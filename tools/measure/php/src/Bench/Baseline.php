<?php

declare(strict_types=1);

namespace Opcov\Measure\Bench;

use Opcov\Measure\Driver;
use Opcov\Measure\Json;

final readonly class Baseline
{
    public const array METRICS = ['instructions', 'cycles'];

    public function __construct(private string $path)
    {
    }

    public function store(string $php, BenchReport $report): void
    {
        $data = is_file($this->path) ? Json::read($this->path) : [];
        $ratios = [];
        foreach (self::METRICS as $metric) {
            $ratios[$metric] = $data[$php]['opcov_over_none'][$metric] ?? [];
            foreach ($report->workloads as $name => $result) {
                $ratios[$metric][$name] = round(self::ratio($name, $result, $metric), 4);
            }
            ksort($ratios[$metric]);
        }
        $data[$php] = ['machine' => $report->toArray()['machine'], 'opcov_over_none' => $ratios];
        ksort($data);
        Json::write($this->path, $data);
    }

    /** @return list<string> */
    public function regressions(string $php, BenchReport $report, float $tolerance): array
    {
        $stored = Json::read($this->path)[$php]['opcov_over_none'] ?? throw new \RuntimeException("$this->path has no baseline for PHP $php; record one with --update-baseline");
        $regressions = [];
        foreach (self::METRICS as $metric) {
            foreach ($report->workloads as $name => $result) {
                $baseline = $stored[$metric][$name] ?? throw new \RuntimeException("$this->path has no PHP $php $metric baseline for workload $name");
                $current = self::ratio($name, $result, $metric);
                if ($current > $baseline * $tolerance) {
                    $regressions[] = sprintf('%s: opcov runs %.3fx the %s of no coverage, the baseline is %.3fx (tolerance %.0f%%)', $name, $current, $metric, $baseline, ($tolerance - 1) * 100);
                }
            }
        }
        return $regressions;
    }

    private static function ratio(string $name, WorkloadResult $result, string $metric): float
    {
        return $result->ratio(Driver::Opcov, static fn (Sample $sample): ?float => $sample->counters->values[$metric] ?? null)
            ?? throw new \RuntimeException("$name has no $metric ratio of opcov over no coverage: both drivers must run and perf must count $metric");
    }
}
