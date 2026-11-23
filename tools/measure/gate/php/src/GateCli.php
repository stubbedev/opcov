<?php

declare(strict_types=1);

namespace Opcov\Measure\Gate;

use Opcov\Measure\Arguments;
use Opcov\Measure\Cli;
use Opcov\Measure\Config;
use Opcov\Measure\DocSection;
use Opcov\Measure\Driver;
use Opcov\Measure\Json;
use Opcov\Measure\Toolchain;

final class GateCli
{
    /** @param list<string> $argv */
    public static function main(array $argv): int
    {
        try {
            if (getenv(Config::VARIABLE) !== false) {
                throw new \LogicException(Config::VARIABLE . ' is set: tools/measure/gate is running inside a measured child and refuses to recurse');
            }
            $arguments = Arguments::parse($argv, ['--env', '--opcov', '--cli', '--rounds', '--budget'], ['--write-docs']);
            $root = dirname(__DIR__, 5);
            $toolchain = Toolchain::open($arguments->required('--env'), $arguments->required('--opcov'), $root);
            $branch = Branch::load($root);
            return match ($arguments->command) {
                'collect' => self::collect($arguments, $toolchain, $branch),
                'compare' => self::compare($arguments, $toolchain, $branch),
                default => throw new \InvalidArgumentException("unknown command {$arguments->command}; expected collect or compare"),
            };
        } catch (\Throwable $e) {
            $message = $e->getMessage();
            for ($previous = $e->getPrevious(); $previous !== null; $previous = $previous->getPrevious()) {
                $message .= ': ' . $previous->getMessage();
            }
            fwrite(STDERR, "tools/measure/gate: $message\n");
            return Cli::ERROR;
        }
    }

    private static function collect(Arguments $arguments, Toolchain $toolchain, Branch $branch): int
    {
        $cli = realpath($arguments->required('--cli'));
        if ($cli === false || !is_executable($cli)) {
            throw new \InvalidArgumentException("the opcov command-line tool {$arguments->required('--cli')} is not an executable file");
        }
        (new Collection($toolchain, $branch, $cli, $arguments->positiveInt('--rounds', 3), $arguments->positiveInt('--budget', 3600)))->execute();
        return Cli::PASSED;
    }

    private static function compare(Arguments $arguments, Toolchain $toolchain, Branch $branch): int
    {
        $workspace = Workspace::open($branch, $toolchain);
        $comparison = Comparison::of(Json::read($workspace->opcovGate()), Json::read($workspace->referenceGate()), Json::read($workspace->opcovReport()));
        $verdict = Explanations::load("$toolchain->root/tools/measure/gate/explanations.json")->judge($comparison->findings);
        $report = new GateReport($branch, self::versions($toolchain, $workspace), Json::read($workspace->timings()), $comparison, $verdict);
        $markdown = $report->markdown();
        echo $markdown;
        if ($arguments->flag('--write-docs')) {
            DocSection::replace("$toolchain->root/docs/gate.md", 'gate:results', $markdown);
        }
        return $report->passed() ? Cli::PASSED : Cli::FAILED;
    }

    /** @return array<string, string> */
    private static function versions(Toolchain $toolchain, Workspace $workspace): array
    {
        $versions = [
            'PHP' => $toolchain->phpRelease,
            'opcov' => substr(Driver::Opcov->version($toolchain), strlen('opcov ')),
            'Xdebug' => substr(Driver::XdebugBranch->version($toolchain), strlen('xdebug ')),
        ];
        $installed = Json::read("$workspace->project/vendor/composer/installed.json");
        foreach ($installed['packages'] ?? [] as $package) {
            if (in_array($package['name'] ?? null, ['phpunit/phpunit', 'phpunit/php-code-coverage'], true)) {
                $versions[$package['name']] = sprintf('%s@%s', $package['version'], substr($package['source']['reference'], 0, 12));
            }
        }
        return $versions;
    }
}
