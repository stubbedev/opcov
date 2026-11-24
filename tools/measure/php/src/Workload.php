<?php

declare(strict_types=1);

namespace Opcov\Measure;

final readonly class Workload
{
    private function __construct(
        public string $sources,
        public string $runner,
    ) {
    }

    public static function open(string $sources, string $runner): self
    {
        $resolvedSources = realpath($sources);
        $resolvedRunner = realpath($runner);
        if ($resolvedSources === false || !is_dir($resolvedSources)) {
            throw new \InvalidArgumentException("workload sources $sources are not a directory");
        }
        if ($resolvedRunner === false || !is_file($resolvedRunner)) {
            throw new \InvalidArgumentException("workload runner $runner does not exist");
        }
        if (str_starts_with($resolvedRunner, "$resolvedSources/")) {
            throw new \InvalidArgumentException("workload runner $runner lies inside the measured sources $sources");
        }
        return new self($resolvedSources, $resolvedRunner);
    }

    /** @return list<\Closure> */
    public function tests(): array
    {
        $tests = require $this->runner;
        if (!is_array($tests) || !array_is_list($tests) || $tests === []) {
            throw new \UnexpectedValueException("$this->runner must return a non-empty list of closures");
        }
        foreach ($tests as $index => $test) {
            if (!$test instanceof \Closure) {
                throw new \UnexpectedValueException("$this->runner test $index is " . get_debug_type($test) . ', not a closure');
            }
        }
        return $tests;
    }

    /** @return array<string, mixed> */
    public function load(): array
    {
        $variables = [];
        foreach (Files::php($this->sources) as $file) {
            $defined = (static function (): array {
                require_once func_get_arg(0);
                return get_defined_vars();
            })($file);
            $clashes = array_intersect_key($defined, $variables);
            if ($clashes !== []) {
                throw new \UnexpectedValueException(sprintf('%s defines $%s, which another measured file of %s defines too', $file, implode(', $', array_keys($clashes)), $this->sources));
            }
            $variables += $defined;
        }
        return $variables;
    }
}
