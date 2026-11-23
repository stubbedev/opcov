--TEST--
write_report() with absolute_paths keys files and function keys by absolute path and records no root; relative output paths and roots are resolved against the working directory
--INI--
opcov.enabled=1
opcov.collect_paths={PWD}/fixtures/app
--FILE--
<?php
require __DIR__ . '/fixtures/report.inc';
$app = __DIR__ . '/fixtures/app';
require "$app/calc.php";
opcov_run_tests();
$dir = opcov_report_dir();
chdir($dir);
\opcov\write_report('report.json', ['absolute_paths' => true]);
$report = json_decode(file_get_contents("$dir/report.json"), true);
var_dump($report['paths']);
echo implode("\n", array_keys($report['files'])), "\n";
echo implode("\n", array_keys($report['files']["$app/calc.php"]['functions'])), "\n";
chdir(__DIR__ . '/fixtures');
\opcov\write_report("$dir/relative.json", ['absolute_paths' => false, 'root' => './app/../app']);
$relative = json_decode(file_get_contents("$dir/relative.json"), true);
var_dump($relative['paths']['root'] === realpath($app), array_keys($relative['files']));
opcov_remove_dir($dir);
--EXPECTF--
array(1) {
  ["mode"]=>
  string(8) "absolute"
}
%s/fixtures/app/calc.php
{main}@%s/fixtures/app/calc.php
add
sign
{closure}@%s/fixtures/app/calc.php:15-15
bool(true)
array(1) {
  [0]=>
  string(8) "calc.php"
}
