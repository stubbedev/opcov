<?php

declare(strict_types=1);

namespace Opcov\Measure\Gate;

use Opcov\Measure\Bench\Stats;
use Opcov\Measure\DocSection;

final readonly class GateReport
{
    /**
     * @param array<string, string> $versions
     * @param array<string, mixed> $timings
     * @param array{explained: list<array{finding: Finding, reason: string}>, unexplained: list<Finding>, stale: list<string>} $verdict
     */
    public function __construct(
        private Branch $branch,
        private array $versions,
        private array $timings,
        private Comparison $comparison,
        private array $verdict,
    ) {
    }

    public function passed(): bool
    {
        return $this->verdict['unexplained'] === [] && $this->verdict['stale'] === [];
    }

    public function markdown(): string
    {
        $project = $this->branch->project;
        $markdown = sprintf(
            "Branch: %s, `%s..%s` of [%s](https://github.com/%s) (%d tests at the head). Versions: %s; opcov built from commit %s. Machine at the start of the collection: %s.\n\n",
            $this->branch->description,
            substr($this->branch->base, 0, 12),
            substr($this->branch->head, 0, 12),
            $project->repository,
            $project->repository,
            $this->timings['tests'],
            implode(', ', array_map(static fn (string $name, string $version): string => "$name $version", array_keys($this->versions), $this->versions)),
            $this->timings['opcov_commit'],
            implode(', ', array_map(static fn (string $name, string $value): string => "$name $value", array_keys($this->timings['machine']), $this->timings['machine'])),
        );
        $markdown .= sprintf("#### Coverage step, median of %d runs\n\n", count($this->timings['rounds']));
        $markdown .= DocSection::table(['step', 'median s', 'min s', 'max s', 'x no coverage', 'peak RSS MiB'], $this->timingRows());
        $markdown .= sprintf("\nXdebug path coverage takes %.1fx as long as opcov.\n\n", $this->median(Step::Xdebug)->median / $this->median(Step::Opcov)->median);
        $gates = array_map(static fn (array $seconds): Stats => Stats::of($seconds), $this->timings['gate_seconds']);
        $markdown .= sprintf(
            "Evaluating the gate from the finished coverage (median): `opcov gate` %.3f s, the Xdebug reference gate %.3f s (loading the `.cov` file).\n\n",
            $gates['opcov']->median,
            $gates['xdebug']->median,
        );
        $markdown .= sprintf(
            "#### Findings\n\n%d changed functions checked by both gates. Xdebug's path coverage enumerated %d paths through them, %d never taken; opcov has no paths and they are not compared.\n\n",
            $this->comparison->functions,
            $this->comparison->paths,
            $this->comparison->pathsNeverTaken,
        );
        $rows = [];
        foreach ($this->comparison->counts() as $category => $count) {
            if ($count === 0) {
                continue;
            }
            $category = Category::from($category);
            $rows[] = [$category->description(), $category->side()->value, (string) $count, $category->explainedByRule() ? ($category->side() === Side::Both ? 'agree' : 'by rule') : 'per entry'];
        }
        $markdown .= DocSection::table(['finding', 'reported by', 'count', 'explained'], $rows);
        if ($this->verdict['explained'] !== []) {
            $markdown .= "\nDifferences explained per entry (`tools/measure/gate/explanations.json`):\n\n";
            $markdown .= DocSection::table(['where', 'difference', 'reason'], array_map(
                static fn (array $entry): array => [
                    sprintf('%s:%d %s', $entry['finding']->file, $entry['finding']->line, $entry['finding']->function),
                    $entry['finding']->category->description() . ($entry['finding']->detail === '' ? '' : " ({$entry['finding']->detail})"),
                    $entry['reason'],
                ],
                $this->verdict['explained'],
            ));
        }
        $markdown .= sprintf(
            "\nVerdict: %s. %d unexplained difference(s), %d stale explanation(s).\n",
            $this->passed() ? 'every difference is explained' : 'FAILED',
            count($this->verdict['unexplained']),
            count($this->verdict['stale']),
        );
        foreach ($this->verdict['unexplained'] as $finding) {
            $markdown .= "\n- unexplained: " . $finding->describe();
        }
        foreach ($this->verdict['stale'] as $stale) {
            $markdown .= "\n- stale: $stale";
        }
        return $markdown . "\n";
    }

    /** @return list<list<string>> */
    private function timingRows(): array
    {
        $none = $this->median(Step::None)->median;
        $rows = [];
        foreach (Step::cases() as $step) {
            $time = $this->median($step);
            $rss = Stats::of(array_map(static fn (array $round): int => $round[$step->value]['peak_rss_bytes'], $this->timings['rounds']));
            $rows[] = [$step->label(), sprintf('%.1f', $time->median), sprintf('%.1f', $time->min), sprintf('%.1f', $time->max), sprintf('%.2f', $time->median / $none), sprintf('%.0f', $rss->median / 1048576)];
        }
        return $rows;
    }

    private function median(Step $step): Stats
    {
        return Stats::of(array_map(static fn (array $round): float => $round[$step->value]['seconds'], $this->timings['rounds']));
    }
}
