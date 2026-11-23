--TEST--
A measured file outside the root fails write_report() with a RuntimeException naming it, and the existing target stays untouched
--INI--
opcov.enabled=1
opcov.collect_paths={PWD}/fixtures/app:{PWD}/fixtures/outside
--FILE--
<?php
require __DIR__ . '/fixtures/report.inc';
require __DIR__ . '/fixtures/app/calc.php';
require __DIR__ . '/fixtures/outside/far.php';
opcov_run_tests();
$dir = opcov_report_dir();
file_put_contents("$dir/report.json", "previous\n");
try {
    \opcov\write_report("$dir/report.json", ['root' => __DIR__ . '/fixtures/app']);
} catch (RuntimeException $e) {
    echo get_class($e), ': ', str_replace(__DIR__, '{DIR}', $e->getMessage()), "\n";
}
var_dump(file_get_contents("$dir/report.json"), scandir($dir));
opcov_remove_dir($dir);
--EXPECTF--
RuntimeException: opcov\write_report(): cannot build the report for %s/report.json: measured file {DIR}/fixtures/outside/far.php lies outside the report root {DIR}/fixtures/app
string(9) "previous
"
array(3) {
  [0]=>
  string(1) "."
  [1]=>
  string(2) ".."
  [2]=>
  string(11) "report.json"
}
