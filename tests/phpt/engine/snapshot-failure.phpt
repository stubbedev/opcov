--TEST--
A file whose control-flow model cannot be built is skipped with the reason and the snapshot-failed code in get_status(); collection stays on, stop(), executable() and write_report() leave that file out and report every other file
--SKIPIF--
<?php
if (!function_exists('opcov\selftest_fail_snapshot')) {
    die('skip needs the engine-selftest build');
}
?>
--INI--
opcov.enabled=1
opcov.collect_paths={PWD}/fixtures
--FILE--
<?php
$failing = __DIR__ . '/fixtures/work.php';
$kept = __DIR__ . '/fixtures/kept.php';
$strip = static fn (string $text): string => preg_replace('~ \(see https://\S+#([a-z-]+)\)~', ' [$1]', str_replace(__DIR__ . '/fixtures/', '', $text));
\opcov\selftest_fail_snapshot();
require $failing;
require $kept;
\opcov\start();
work(2);
kept();
$hits = \opcov\stop('T::t');
\opcov\reset();
$status = \opcov\get_status();
var_dump($status['off_reason']);
echo 'selected: ', implode(', ', array_map($strip, $status['selected_files'])), "\n";
foreach ($status['failed_files'] as $file => $failure) {
    echo 'failed: ', $strip($file), ': ', $failure['code'], ': ', $strip($failure['reason']), "\n";
}
echo 'files_failed: ', $status['counters']['files_failed'], "\n";
echo 'hits: ', implode(', ', array_map($strip, array_keys($hits))), "\n";
echo 'executable: ', implode(', ', array_map($strip, array_keys(\opcov\executable([$failing, $kept])))), "\n";
echo 'debug: ', $strip(\opcov\get_debug_info($failing)['skip_reason']), "\n";
\opcov\ensure_can_collect();
$out = sys_get_temp_dir() . '/opcov-snapshot-failure-' . getmypid() . '.json';
\opcov\write_report($out, ['root' => __DIR__ . '/fixtures']);
echo 'report: ', implode(', ', array_keys(json_decode(file_get_contents($out), true)['files'])), "\n";
unlink($out);
--EXPECT--
NULL
selected: kept.php
failed: work.php: snapshot-failed: cannot analyse code compiled from work.php: failure injected by opcov\selftest_fail_snapshot() [snapshot-failed]
files_failed: 1
hits: kept.php
executable: kept.php
debug: cannot analyse code compiled from work.php: failure injected by opcov\selftest_fail_snapshot() [snapshot-failed]
report: kept.php
