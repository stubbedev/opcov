--TEST--
A file with declarations only has no executable line (its main code runs only declarations and the final RETURN), so no test is credited with loading it; abstract methods have no lines
--INI--
opcov.enabled=1
opcov.collect_paths={PWD}/fixtures
--FILE--
<?php
require __DIR__ . '/../support/coverage.inc';
echo opcov_lines(__DIR__ . "/fixtures/declarations.php", static function (): void {
    require __DIR__ . "/fixtures/declarations.php";
});
echo opcov_lines(__DIR__ . "/fixtures/declarations.php", static function (): void {
    var_dump((new Square(2.0))->area());
});
--EXPECT--
declarations.php
  covered: 
  uncovered: 9,11,15
float(4)
declarations.php
  covered: 9,11,15
  uncovered: 
