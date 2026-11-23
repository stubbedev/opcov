<?php

declare(strict_types=1);

namespace Opcov\Measure\Bench;

use Opcov\Measure\CorpusProject;
use Opcov\Measure\CorpusTarget;
use Opcov\Measure\StandaloneTarget;
use Opcov\Measure\Target;
use Opcov\Measure\Toolchain;
use Opcov\Measure\Workload;

enum BenchWorkload: string
{
    case Branchy = 'branchy';
    case TinyTests = 'tiny-tests';
    case Carbon = 'carbon';
    case Laravel = 'laravel-query-builder';

    public function description(): string
    {
        return match ($this) {
            self::Branchy => 'synthetic branch-heavy CPU loop: one collection around 400k calls of a function with if/elseif, &&/||, ??, ?:, match, switch, foreach and ?->',
            self::TinyTests => '20,000 tiny tests touching the same class, each bracketed by start/stop and the driver\'s per-test reset (opcov `reset()`, PCOV `clear()`)',
            self::Carbon => 'Carbon\'s PHPUnit suite (large unit suite, many small tests, `src/Carbon` measured)',
            self::Laravel => 'spatie/laravel-query-builder\'s Pest suite: Laravel feature tests through Orchestra Testbench on in-memory SQLite (`src` measured)',
        };
    }

    public function target(Toolchain $toolchain): Target
    {
        return match ($this) {
            self::Branchy, self::TinyTests => new StandaloneTarget(
                "bench/$this->value",
                Workload::open("$toolchain->root/bench/workloads/$this->value/src", "$toolchain->root/bench/workloads/$this->value/run.php"),
                null,
            ),
            self::Carbon => CorpusTarget::prepare(CorpusProject::named($toolchain->root, 'carbon'), $toolchain),
            self::Laravel => CorpusTarget::prepare(CorpusProject::named($toolchain->root, 'laravel-query-builder'), $toolchain),
        };
    }

    public static function parse(string $name): self
    {
        return self::tryFrom($name) ?? throw new \InvalidArgumentException(sprintf(
            'unknown workload %s; expected one of %s',
            $name,
            implode(', ', array_map(static fn (self $workload): string => $workload->value, self::cases())),
        ));
    }
}
