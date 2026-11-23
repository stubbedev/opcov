--TEST--
A generator resumed after stop() and reset() reports its later lines in the next collection
--INI--
opcov.enabled=1
opcov.collect_paths={PWD}/fixtures
--FILE--
<?php
require __DIR__ . '/../support/coverage.inc';
require __DIR__ . "/fixtures/generator.php";
$numbers = numbers();
echo opcov_lines(__DIR__ . "/fixtures/generator.php", static function () use ($numbers): void {
    $numbers->current();
});
echo opcov_lines(__DIR__ . "/fixtures/generator.php", static function () use ($numbers): void {
    $numbers->next();
    $numbers->next();
});
--EXPECT--
generator.php
  covered: 4
  uncovered: 2,5,6,7,8
generator.php
  covered: 5,6
  uncovered: 2,4,7,8
