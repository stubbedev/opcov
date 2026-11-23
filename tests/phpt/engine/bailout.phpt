--TEST--
A fatal error raised inside opcov's own code (memory exhaustion while get_debug_info() builds its array) is caught at the engine call, every Rust frame returns normally, and the bailout resumes from the entry point: opcov's state stays usable in shutdown functions, and the report is still written
--SKIPIF--
<?php
if (getenv('USE_ZEND_ALLOC') === '0') {
    die('skip memory_limit is not enforced without the Zend allocator (USE_ZEND_ALLOC=0), so the memory exhaustion under test cannot happen');
}
?>
--INI--
opcov.enabled=1
opcov.collect_paths={PWD}/fixtures
memory_limit=64M
--FILE--
<?php
$file = __DIR__ . '/fixtures/generated-' . getmypid() . '.php';
$source = "<?php\nfunction big(int \$n): int\n{\n    \$total = 0;\n";
for ($i = 0; $i < 400; $i++) {
    $source .= "    if (\$n > $i) {\n        \$total += $i;\n    }\n";
}
file_put_contents($file, $source . "    return \$total;\n}\n");
require $file;
\opcov\start();
big(3);
\opcov\stop('Before::test');
register_shutdown_function(static function () use ($file): void {
    $GLOBALS['padding'] = null;
    $status = \opcov\get_status();
    echo "off reason: ", json_encode($status['off_reason']), ", collecting: ", json_encode($status['collecting']), "\n";
    $before = memory_get_usage();
    $info = \opcov\get_debug_info($file);
    echo "debug info needs more than the 2 MiB left: ", json_encode(memory_get_usage() - $before > 2 * 1024 * 1024), "\n";
    echo "debug info probes: ", count($info['probes']) > 0 ? 'yes' : 'no', "\n";
    unset($info);
    \opcov\start();
    big(1);
    \opcov\stop('After::test');
    $report = tempnam(sys_get_temp_dir(), 'opcov-report-');
    \opcov\write_report($report, ['absolute_paths' => true]);
    echo "tests: ", json_encode(json_decode(file_get_contents($report), true)['tests']), "\n";
    unlink($report);
    unlink($file);
});
$padding = new SplFixedArray(100000);
for ($i = 0; memory_get_usage(true) < 64 * 1024 * 1024; $i++) {
    $padding[$i] = str_repeat('x', 999);
}
\opcov\get_debug_info($file);
echo "not reached\n";
?>
--EXPECTF--

Fatal error: Allowed memory size of 67108864 bytes exhausted%s(tried to allocate %d bytes) in %sbailout.php on line 34
off reason: null, collecting: false
debug info needs more than the 2 MiB left: true
debug info probes: yes
tests: ["After::test","Before::test"]
