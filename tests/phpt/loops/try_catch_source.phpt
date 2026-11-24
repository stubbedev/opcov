--TEST--
opcov doctor --report maps every outcome of the try/catch loops, so the loop test of each loop matches its loop construct
--SKIPIF--
<?php
if ((string) getenv('OPCOV_CLI') === '') {
    die('skip OPCOV_CLI must name the opcov binary');
}
?>
--INI--
opcov.enabled=1
opcov.collect_paths={PWD}/fixtures
--FILE--
<?php
$fixtures = __DIR__ . '/fixtures';
require "$fixtures/try_catch.php";
$report = tempnam(sys_get_temp_dir(), 'opcov-try-catch-');
\opcov\write_report($report, ['root' => $fixtures]);
$process = proc_open([getenv('OPCOV_CLI'), 'doctor', '--report', $report, '--format', 'json'], [1 => ['pipe', 'w'], 2 => ['pipe', 'w']], $pipes);
$output = stream_get_contents($pipes[1]);
$errors = stream_get_contents($pipes[2]);
fclose($pipes[1]);
fclose($pipes[2]);
echo 'exit ', proc_close($process), $errors, "\n";
unlink($report);
$diagnosis = json_decode($output, true, flags: JSON_THROW_ON_ERROR);
$loops = 0;
foreach (\opcov\executable(["$fixtures/try_catch.php"])["$fixtures/try_catch.php"]['functions'] as $function) {
    foreach ($function['outcomes'] as $outcome) {
        $loops += in_array($outcome['kind'], ['LoopBack', 'LoopExit'], true) ? 1 : 0;
    }
}
echo "outcomes: {$diagnosis['outcomes']}, loop tests: $loops, unmatched: {$diagnosis['unmatched']}\n";
foreach ($diagnosis['files'] as $file) {
    foreach ($file['unmatched_outcomes'] as $outcome) {
        echo "  {$outcome['function']} ", json_encode($outcome['kind']), "@{$outcome['line']}: {$outcome['reason']}\n";
    }
}
?>
--EXPECT--
exit 0
outcomes: 25, loop tests: 12, unmatched: 0