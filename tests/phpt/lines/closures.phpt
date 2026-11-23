--TEST--
Closures defined in a loop share one unit and report each line once
--INI--
opcov.enabled=1
opcov.collect_paths={PWD}/fixtures
--FILE--
<?php
require __DIR__ . '/../support/coverage.inc';
$kept = [];
echo opcov_lines(__DIR__ . "/fixtures/closures.php", static function () use (&$kept): void {
    $kept = (static function (): array {
        require __DIR__ . "/fixtures/closures.php";
        return [$squares, $double];
    })();
});
$units = array_count_values(array_column(\opcov\get_debug_info(__DIR__ . "/fixtures/closures.php")["probes"], "function"));
ksort($units);
foreach ($units as $function => $probes) {
    echo preg_replace("~@.*/~", "@", $function), ": $probes probes\n";
}
--EXPECT--
closures.php
  covered: 2,3,4,5,8,9,10,11
  uncovered: 
{closure}@closures.php:4-6: 2 probes
{closure}@closures.php:9-9: 1 probes
