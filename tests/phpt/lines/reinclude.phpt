--TEST--
A file included twice is compiled twice; both copies report into the same file
--INI--
opcov.enabled=1
opcov.collect_paths={PWD}/fixtures
--FILE--
<?php
require __DIR__ . '/../support/coverage.inc';
echo opcov_lines(__DIR__ . "/fixtures/included.php", static function (): void {
    include __DIR__ . "/fixtures/included.php";
});
echo opcov_lines(__DIR__ . "/fixtures/included.php", static function (): void {
    $counter = 1;
    include __DIR__ . "/fixtures/included.php";
});
--EXPECT--
included.php
  covered: 2,3,6
  uncovered: 4
again
included.php
  covered: 2,3,4,6
  uncovered:
