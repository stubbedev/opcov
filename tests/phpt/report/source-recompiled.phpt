--TEST--
A measured file that PHP compiles twice from different bytes in one process fails write_report() with a RuntimeException naming it, since no single sha256 describes its coverage; compiling it again from the same bytes is fine
--INI--
opcov.enabled=1
opcov.collect_paths={PWD}/fixtures/mutable
--FILE--
<?php
require __DIR__ . '/fixtures/report.inc';
$mutable = __DIR__ . '/fixtures/mutable';
$file = "$mutable/recompiled-" . getmypid() . '.php';
$dir = opcov_report_dir();
$write = static function () use ($dir, $mutable, $file): void {
    try {
        \opcov\write_report("$dir/report.json", ['root' => $mutable]);
        echo "written\n";
    } catch (RuntimeException $e) {
        echo get_class($e), ': ', str_replace([$dir, $file], ['{OUT}', '{FILE}'], $e->getMessage()), "\n";
    }
};
file_put_contents($file, "<?php\nreturn 1;\n");
var_dump(include $file);
var_dump(include $file);
$write();
file_put_contents($file, "<?php\nreturn 2;\n");
var_dump(include $file);
file_put_contents($file, "<?php\nreturn 1;\n");
$write();
unlink($file);
opcov_remove_dir($dir);
--EXPECT--
int(1)
int(1)
written
int(2)
RuntimeException: opcov\write_report(): cannot build the report for {OUT}/report.json: {FILE} was compiled from different sources in this process, so no single sha256 describes its coverage
--CLEAN--
<?php
foreach (glob(__DIR__ . '/fixtures/mutable/recompiled-*.php') as $leftover) {
    unlink($leftover);
}
