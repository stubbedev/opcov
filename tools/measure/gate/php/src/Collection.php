<?php

declare(strict_types=1);

namespace Opcov\Measure\Gate;

use Opcov\Measure\Bench\Machine;
use Opcov\Measure\BudgetExceeded;
use Opcov\Measure\Json;
use Opcov\Measure\Process;
use Opcov\Measure\ProcessResult;
use Opcov\Measure\Toolchain;

final readonly class Collection
{
    private const array ACCEPTED_TEST_EXITS = [0, 1, 2];

    private const array ACCEPTED_GATE_EXITS = [0, 1];

    public function __construct(
        private Toolchain $toolchain,
        private Branch $branch,
        private string $cli,
        private int $rounds,
        private int $budgetSeconds,
    ) {
    }

    public function execute(): void
    {
        $machine = Machine::describe();
        $commit = Provenance::opcovCommit($this->toolchain);
        $workspace = Workspace::prepare($this->branch, $this->toolchain);
        $diff = $this->branch->diff($this->toolchain);
        $rounds = [];
        $tests = null;
        for ($round = 1; $round <= $this->rounds; $round++) {
            foreach (Step::cases() as $step) {
                fwrite(STDERR, "gate: {$step->value} round $round/$this->rounds\n");
                $output = $step->output($workspace);
                if ($output !== null && is_file($output) && !unlink($output)) {
                    throw new \RuntimeException("cannot remove the stale output $output");
                }
                $result = $this->run($step->command($this->toolchain, $workspace), $step->environment($this->toolchain, $workspace), $workspace->project, $workspace->log("{$step->value}-$round"), "the test suite under {$step->value}", self::ACCEPTED_TEST_EXITS);
                if ($output !== null && !is_file($output)) {
                    throw new \RuntimeException("the test suite under {$step->value} wrote no $output; log: $result->log");
                }
                $count = self::testCount($result);
                if ($tests !== null && $count !== $tests) {
                    throw new \RuntimeException("the test suite under {$step->value} ran $count tests, an earlier run $tests; log: $result->log");
                }
                $tests = $count;
                $rounds[$round][$step->value] = ['seconds' => $result->seconds, 'peak_rss_bytes' => $result->peakRssBytes, 'exit' => $result->exit];
            }
        }
        $gates = [];
        for ($round = 1; $round <= $this->rounds; $round++) {
            fwrite(STDERR, "gate: evaluation round $round/$this->rounds\n");
            $gates['opcov'][] = $this->run([$this->cli, 'gate', '--report', $workspace->opcovReport(), '--diff', $diff, '--format', 'json'], [], $workspace->project, $workspace->opcovGate(), 'opcov gate', self::ACCEPTED_GATE_EXITS)->seconds;
            $gates['xdebug'][] = $this->run([$this->toolchain->php(), '-d', 'memory_limit=-1', dirname(__DIR__) . '/reference.php', $workspace->project, $workspace->xdebugCoverage(), $diff, $workspace->referenceGate()], [], $workspace->project, $workspace->log('reference-gate'), 'the Xdebug reference gate', [0])->seconds;
        }
        Json::write($workspace->timings(), [
            'machine' => $machine,
            'opcov_commit' => $commit,
            'tests' => $tests,
            'rounds' => array_values($rounds),
            'gate_seconds' => $gates,
        ]);
    }

    /**
     * @param list<string> $command
     * @param array<string, string> $environment
     * @param list<int> $accepted
     */
    private function run(array $command, array $environment, string $cwd, string $log, string $what, array $accepted): ProcessResult
    {
        $result = Process::run($this->toolchain, $command, $environment, $cwd, $log, $this->budgetSeconds);
        if ($result->timedOut()) {
            throw new BudgetExceeded("$what exceeded its budget of {$this->budgetSeconds}s; log: $log");
        }
        if (!in_array($result->exit, $accepted, true)) {
            throw new \RuntimeException("$what exited with $result->exit; last output:\n" . $result->tail());
        }
        return $result;
    }

    private static function testCount(ProcessResult $result): int
    {
        $output = @file_get_contents($result->log);
        if ($output === false) {
            throw new \RuntimeException("cannot read $result->log");
        }
        if (preg_match('~^(?:OK \((\d+) tests?,|Tests: (\d+),)~m', $output, $match) !== 1) {
            throw new \RuntimeException("$result->log has no PHPUnit summary line");
        }
        return (int) ($match[1] !== '' ? $match[1] : $match[2]);
    }
}
