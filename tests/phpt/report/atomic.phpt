--TEST--
A report that cannot be renamed into place or created fails with a RuntimeException naming the path; the target is untouched and no temporary file is left
--INI--
opcov.enabled=1
opcov.collect_paths={PWD}/fixtures/app
--FILE--
<?php
require __DIR__ . '/fixtures/report.inc';
require __DIR__ . '/fixtures/app/calc.php';
opcov_run_tests();
$dir = opcov_report_dir();
mkdir("$dir/target");
file_put_contents("$dir/target/kept", "kept\n");
foreach (["$dir/target", "$dir/missing/report.json"] as $target) {
    try {
        \opcov\write_report($target, ['root' => __DIR__]);
    } catch (RuntimeException $e) {
        echo get_class($e), ': ', str_replace($dir, '{TMP}', $e->getMessage()), "\n";
    }
}
var_dump(scandir($dir), scandir("$dir/target"), file_get_contents("$dir/target/kept"));
unlink("$dir/target/kept");
rmdir("$dir/target");
opcov_remove_dir($dir);
--EXPECTF--
RuntimeException: opcov\write_report(): cannot write the report to {TMP}/target: cannot rename {TMP}/.target.%d.tmp to {TMP}/target: %s
RuntimeException: opcov\write_report(): cannot write the report to {TMP}/missing/report.json: cannot create {TMP}/missing/.report.json.%d.tmp: No such file or directory (os error 2)
array(3) {
  [0]=>
  string(1) "."
  [1]=>
  string(2) ".."
  [2]=>
  string(6) "target"
}
array(3) {
  [0]=>
  string(1) "."
  [1]=>
  string(2) ".."
  [2]=>
  string(4) "kept"
}
string(5) "kept
"
