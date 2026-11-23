--TEST--
Both outcomes of a source in one collection, outcomes inside a closure, a generator, a fiber and a finally block, and exceptions thrown inside the condition or between a branch and its successor never misattribute an outcome
--INI--
opcov.enabled=1
opcov.collect_paths={PWD}/fixtures
--FILE--
<?php
require __DIR__ . '/../support/coverage.inc';
$file = __DIR__ . '/fixtures/contexts.php';
$closure = require $file;
$calls = [
    'both outcomes in one collection' => static function () use ($closure): void {
        $closure(1);
        $closure(null);
        foreach (generate(1) as $_) {
        }
        foreach (generate(null) as $_) {
        }
    },
    'closure' => static fn () => $closure(null),
    'generator resumed' => static function (): void {
        foreach (generate(1) as $_) {
        }
    },
    'fiber' => static fn () => in_fiber(0),
    'finally' => static fn () => in_finally(1),
    'exception in the smart-branch condition' => static fn () => guarded(new stdClass(), 42),
    'condition decided' => static fn () => guarded(new stdClass(), stdClass::class),
    'undefined variable, error handler throws' => static function (): void {
        set_error_handler(static fn () => throw new ErrorException('undefined'));
        try {
            undefined_condition();
        } finally {
            restore_error_handler();
        }
    },
    'undefined variable, error handler returns' => static function (): void {
        set_error_handler(static fn () => true);
        try {
            undefined_condition();
        } finally {
            restore_error_handler();
        }
    },
    'exception between FE_FETCH and its successor' => static function (): void {
        try {
            iterate(throwing());
        } catch (RuntimeException $e) {
            echo "caught\n";
        }
    },
];
foreach ($calls as $name => $call) {
    echo $name, "\n", opcov_branches($file, $call);
}
?>
--EXPECT--
both outcomes in one collection
  generate: CoalesceFallback@8 CoalesceValue@8 IfTrue@9 IfFalse@9
  {closure}@contexts.php:2-4: CoalesceFallback@3 CoalesceValue@3
closure
  {closure}@contexts.php:2-4: CoalesceFallback@3
generator resumed
  generate: CoalesceValue@8 IfTrue@9
fiber
  {closure}@contexts.php:16-19: CoalesceValue@17 IfFalse@18
finally
finally then
  in_finally: Finally@28 IfTrue@30
exception in the smart-branch condition
  guarded: Catch@43
condition decided
  guarded: IfTrue@39
undefined variable, error handler throws
  undefined_condition: Catch@55
undefined variable, error handler returns
  undefined_condition: IfFalse@51
exception between FE_FETCH and its successor
item
caught
  iterate: LoopEnter@62 LoopBack@62 IfTrue@63
