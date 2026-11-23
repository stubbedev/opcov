--TEST--
Language features: __destruct running while an exception unwinds, a generator suspended between a branch and its successor, declare(ticks), include inside a function (the included file measured per include), static fn and first-class callables, property hooks and a lazy ghost initialised on first access
--INI--
opcov.enabled=1
opcov.collect_paths={PWD}/fixtures
--FILE--
<?php
require __DIR__ . '/../support/coverage.inc';
$file = __DIR__ . '/fixtures/features.php';
$included = __DIR__ . '/fixtures/included.php';
require $file;
echo "== unwinding\n";
echo opcov_branches($file, static fn () => unwind(true));
echo opcov_branches($file, static fn () => unwind(false));
echo "== generator\n";
$generator = suspended(true);
echo opcov_branches($file, static function () use ($generator): void {
    echo $generator->current(), "\n";
});
echo opcov_branches($file, static function () use ($generator): void {
    $generator->next();
});
echo opcov_branches($file, static function (): void {
    foreach (suspended(false) as $value) {
        echo $value, "\n";
    }
});
echo "== ticks\n";
echo opcov_branches($file, static fn () => ticking(3));
echo "== include inside a function\n";
echo opcov_lines($included, static fn () => includes($included, false));
echo opcov_lines($included, static fn () => includes($included, true));
echo "== callables\n";
echo opcov_branches($file, static fn () => callables(2));
echo "== hooks and lazy objects\n";
echo opcov_branches($file, static fn () => lazy(false));
echo opcov_branches($file, static function (): void {
    echo lazy(true), "\n";
});
echo opcov_branches($file, static fn () => new Point(-1));
?>
--EXPECT--
== unwinding
destructed during unwinding
threw RuntimeException
  unwind: IfTrue@20 IfTrue@21
  Noisy::__destruct: IfTrue@10
destructed normally
  unwind: IfFalse@20 IfFalse@21
  Noisy::__destruct: IfFalse@10
== generator
between
  suspended: IfTrue@28
resumed true
  (none)
other
  suspended: IfFalse@28
== ticks
  ticking: LoopBack@40 LoopExit@40 PreTest@40:second
== include inside a function
included.php
  covered: 2,5
  uncovered: 3
included.php
  covered: 2,3
  uncovered: 5
== callables
  {closure}@features.php:58-58: IfTrue@58 IfFalse@58
== hooks and lazy objects
  lazy: IfFalse@88
initialised
x=3
  lazy: IfTrue@88
  Point::$x::set: IfFalse@68
initialised
threw ValueError
  Point::$x::set: IfTrue@68
