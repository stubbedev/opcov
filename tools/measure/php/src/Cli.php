<?php

declare(strict_types=1);

namespace Opcov\Measure;

use Opcov\Measure\Bench\BenchRun;
use Opcov\Measure\Parity\Allowlist;
use Opcov\Measure\Parity\Findings;
use Opcov\Measure\Parity\ParityReport;
use Opcov\Measure\Parity\ParityRun;

final class Cli
{
    public const int PASSED = 0;

    public const int FAILED = 1;

    public const int ERROR = 2;

    /** @param list<string> $argv */
    public static function main(array $argv): int
    {
        try {
            if (getenv(Config::VARIABLE) !== false) {
                throw new \LogicException(Config::VARIABLE . ' is set: tools/measure is running inside one of its own measured children and refuses to recurse');
            }
            $arguments = Arguments::parse($argv, ['--env', '--opcov', '--suite', '--budget', '--runs', '--workload', '--driver'], ['--write-docs', '--check', '--update-baseline']);
            $root = dirname(__DIR__, 4);
            return match ($arguments->command) {
                'parity' => self::parity($arguments, $root),
                'bench' => BenchRun::fromArguments($arguments, self::toolchain($arguments, $root))->execute(),
                'lock' => self::lock($arguments, $root),
                default => throw new \InvalidArgumentException("unknown command {$arguments->command}; expected parity, bench or lock"),
            };
        } catch (\Throwable $e) {
            fwrite(STDERR, sprintf("tools/measure: %s\n", self::describe($e)));
            return self::ERROR;
        }
    }

    private static function describe(\Throwable $e): string
    {
        $message = $e->getMessage();
        for ($previous = $e->getPrevious(); $previous !== null; $previous = $previous->getPrevious()) {
            $message .= ': ' . $previous->getMessage();
        }
        return $message;
    }

    private static function toolchain(Arguments $arguments, string $root): Toolchain
    {
        return Toolchain::open($arguments->required('--env'), $arguments->required('--opcov'), $root);
    }

    private static function parity(Arguments $arguments, string $root): int
    {
        $toolchain = self::toolchain($arguments, $root);
        $run = new ParityRun($toolchain, $arguments->positiveInt('--budget', 3600));
        $suites = new Suites($toolchain);
        $findings = new Findings();
        $labels = [];
        $perSuite = [];
        foreach ($arguments->all('--suite', ['golden', 'fixtures']) as $suite) {
            $suiteFindings = new Findings();
            foreach ($suites->resolve($suite) as $target) {
                fwrite(STDERR, "parity: {$target->label()}\n");
                $suiteFindings->absorb($run->compare($target));
                $labels[] = $target->label();
            }
            $perSuite[$suite] = $suiteFindings;
            $findings->absorb($suiteFindings);
        }
        $verdict = Allowlist::load("$root/tests/diff/allowlist.json")->judge($findings->divergences(), $labels);
        $report = new ParityReport($toolchain, $perSuite, $verdict);
        echo $report->text();
        if ($arguments->flag('--write-docs')) {
            DocSection::replace("$root/docs/parity.md", 'parity:php' . str_replace('.', '', $toolchain->phpVersion()), $report->markdown());
        }
        return $verdict->passed() ? self::PASSED : self::FAILED;
    }

    private static function lock(Arguments $arguments, string $root): int
    {
        $toolchain = self::toolchain($arguments, $root);
        foreach ($arguments->all('--suite', array_keys(CorpusProject::all($root))) as $name) {
            CorpusProject::named($root, $name)->lock($toolchain);
            fwrite(STDERR, "lock: stored tools/measure/corpus/$name.lock\n");
        }
        return self::PASSED;
    }
}
