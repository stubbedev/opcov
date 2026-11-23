--TEST--
Operands on later lines of a multi-line expression are covered only when they run; a consumer line is not probed
--INI--
opcov.enabled=1
opcov.collect_paths={PWD}/fixtures
--FILE--
<?php
require __DIR__ . '/../support/coverage.inc';
require __DIR__ . "/fixtures/multiline.php";
echo opcov_lines(__DIR__ . "/fixtures/multiline.php", static function (): void {
    either(true, false);
    pick(1);
});
echo opcov_lines(__DIR__ . "/fixtures/multiline.php", static function (): void {
    either(false, true);
    pick(3);
});
--EXPECT--
multiline.php
  covered: 4,10,11
  uncovered: 5,12,13,17,18,19
multiline.php
  covered: 4,5,10,13
  uncovered: 11,12,17,18,19
