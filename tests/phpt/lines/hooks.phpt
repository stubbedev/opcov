--TEST--
Property hooks are measured
--INI--
opcov.enabled=1
opcov.collect_paths={PWD}/fixtures
--FILE--
<?php
require __DIR__ . '/../support/coverage.inc';
echo opcov_lines(__DIR__ . "/fixtures/hooks.php", static function (): void {
    require __DIR__ . "/fixtures/hooks.php";
});
--EXPECT--
Ada
hooks.php
  covered: 5,7,8,12,13,14,15
  uncovered:
