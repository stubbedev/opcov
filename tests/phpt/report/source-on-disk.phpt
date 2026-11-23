--TEST--
write_report() hashes each measured file when it writes the report: the sha256 is that of the compiled bytes, a file changed or removed on disk since PHP compiled it fails the report with a RuntimeException naming it, and only its content counts, not its modification time
--INI--
opcov.enabled=1
opcov.collect_paths={PWD}/fixtures/mutable
--FILE--
<?php
require __DIR__ . '/fixtures/report.inc';
$mutable = __DIR__ . '/fixtures/mutable';
$file = "$mutable/changing-" . getmypid() . '.php';
$compiled = "<?php\nreturn static fn (int \$n): int => \$n * 2;\n";
file_put_contents($file, $compiled);
$stable = require "$mutable/stable.php";
$double = require $file;
\opcov\start();
$stable(1);
$double(2);
\opcov\stop('Tests\ChangingTest::test_double');
\opcov\reset();
$dir = opcov_report_dir();
$write = static function () use ($dir, $mutable, $file): void {
    try {
        \opcov\write_report("$dir/report.json", ['root' => $mutable]);
        $report = json_decode(file_get_contents("$dir/report.json"), true);
        $name = basename($file);
        var_dump($report['files'][$name]['sha256'] === hash('sha256', $GLOBALS['compiled']));
        var_dump($report['files']['stable.php']['sha256'] === hash_file('sha256', "$mutable/stable.php"));
    } catch (RuntimeException $e) {
        echo get_class($e), ': ', str_replace([$dir, $file], ['{OUT}', '{FILE}'], $e->getMessage()), "\n";
    }
};
file_put_contents($file, "<?php\nreturn static fn (int \$n): int => \$n * 3;\n");
$write();
file_put_contents($file, $compiled);
touch($file, time() + 60);
$write();
unlink($file);
$write();
opcov_remove_dir($dir);
--EXPECTF--
RuntimeException: opcov\write_report(): cannot build the report for {OUT}/report.json: {FILE} changed on disk after PHP compiled it, so the sha256 of the compiled source cannot be computed; keep measured files unchanged until the report is written
bool(true)
bool(true)
RuntimeException: opcov\write_report(): cannot build the report for {OUT}/report.json: the source of {FILE} could not be hashed: cannot read it to write the report: No such file or directory (os error 2)
--CLEAN--
<?php
foreach (glob(__DIR__ . '/fixtures/mutable/changing-*.php') as $leftover) {
    unlink($leftover);
}
