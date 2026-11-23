--TEST--
A trait method used by two classes shares its opcodes, unit and probes; both classes hit the trait's lines
--INI--
opcov.enabled=1
opcov.collect_paths={PWD}/fixtures
--FILE--
<?php
require __DIR__ . '/../support/coverage.inc';
require __DIR__ . "/fixtures/traits.php";
echo opcov_lines(__DIR__ . "/fixtures/traits.php", static function (): void {
    (new Ann())->greet();
});
echo opcov_lines(__DIR__ . "/fixtures/traits.php", static function (): void {
    (new Bob())->greet();
});
$functions = array_unique(array_column(\opcov\get_debug_info(__DIR__ . "/fixtures/traits.php")["probes"], "function"));
sort($functions);
echo implode(", ", $functions), "\n";
--EXPECT--
traits.php
  covered: 6,16
  uncovered: 26
traits.php
  covered: 6,26
  uncovered: 16
Ann::name, Bob::name, Greets::greet
