<?php

declare(strict_types=1);

namespace Opcov\Measure\Parity;

use Opcov\Measure\Child;
use Opcov\Measure\Driver;
use Opcov\Measure\Mode;
use Opcov\Measure\Target;
use Opcov\Measure\Toolchain;

final readonly class ParityRun
{
    public const array DRIVERS = [Driver::Opcov, Driver::Pcov, Driver::XdebugBranch, Driver::XdebugLines];

    public function __construct(
        private Toolchain $toolchain,
        private int $budgetSeconds,
    ) {
        foreach (self::DRIVERS as $driver) {
            if (!$driver->available($toolchain)) {
                throw new \RuntimeException(sprintf('parity needs %s, which does not build for PHP %s in this environment', $driver->label(), $toolchain->phpVersion()));
            }
        }
    }

    public function compare(Target $target): Findings
    {
        $coverage = [];
        $tests = [];
        $xdebug = $target->xdebug();
        foreach ([Driver::Opcov, Driver::Pcov, $xdebug] as $driver) {
            $output = sprintf('%s/parity/%s/%s/%s.json', $this->toolchain->cache, $this->toolchain->phpVersion(), $target->label(), $driver->value);
            $result = (new Child($this->toolchain, $target, $driver, Mode::Coverage))->run($output, $this->budgetSeconds);
            $coverage[$driver->value] = $result->coverage($output);
            $tests[$driver->value] = $result->tests();
        }
        if (count(array_unique($tests)) !== 1) {
            throw new \RuntimeException(sprintf('%s ran a different number of tests per driver: %s', $target->label(), json_encode($tests)));
        }
        $findings = new Findings();
        $lastLine = static function (string $file) use ($target): int {
            $source = file_get_contents($target->root() . '/' . $file);
            if ($source === false) {
                throw new \RuntimeException("cannot read the measured file {$target->root()}/$file");
            }
            return substr_count($source, "\n");
        };
        $findings->absorb(Comparator::lines($target->label(), $coverage[Driver::Opcov->value], $coverage[Driver::Pcov->value], $xdebug, $coverage[$xdebug->value], $lastLine));
        if ($xdebug === Driver::XdebugBranch) {
            $findings->absorb(Comparator::outcomes($target->label(), $coverage[Driver::Opcov->value], $coverage[$xdebug->value]));
        } else {
            $findings->outcomesNotCompared();
        }
        $expectations = $target->expectations();
        if ($expectations !== null) {
            $findings->absorb(Expectations::load($expectations)->check($target->label(), $coverage[Driver::Opcov->value]));
        }
        return $findings;
    }
}
