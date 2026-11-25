--TEST--
opcov doctor runs the probe self-test in a child PHP that reads the user's INI files: it reports the thread safety of the PHP it ran, passes with opcov loaded, names an unloaded extension, and lists every OPcache conflict with its fix and troubleshooting link
--SKIPIF--
<?php
require __DIR__ . '/../support/coverage.inc';
if ((string) getenv('OPCOV_CLI') === '') {
    die('skip OPCOV_CLI must name the opcov binary');
}
if (opcov_opcache_ini() === null) {
    die('skip OPcache is neither built in nor named by OPCOV_OPCACHE_EXTENSION');
}
?>
--FILE--
<?php
require __DIR__ . '/../support/coverage.inc';
$extension = current(array_filter(opcov_extra_args(), static fn (string $argument): bool => str_starts_with($argument, 'extension=') && str_contains($argument, 'opcov'))) ?: throw new RuntimeException('TEST_PHP_EXTRA_ARGS loads no opcov extension');
$scan = sys_get_temp_dir() . '/opcov-doctor-ini-' . getmypid();
mkdir($scan);
$doctor = static function (string $ini) use ($scan): OpcovProcess {
    file_put_contents($scan . '/opcov.ini', $ini);
    return opcov_cli(['doctor', '--php', PHP_BINARY, '--format', 'json'], ['PHP_INI_SCAN_DIR' => $scan, 'PHPRC' => '']);
};
$summary = static function (OpcovProcess $doctor): string {
    $checkup = json_decode($doctor->output, true);
    return json_encode([
        'exit' => $doctor->status,
        'loaded' => $checkup['runtime'] !== null,
        'thread_safety_matches' => ($checkup['runtime']['zts'] ?? ZEND_THREAD_SAFE) === ZEND_THREAD_SAFE,
        'self_test' => $checkup['self_test'],
        'findings' => array_map(static fn (array $finding): string => $finding['code'] . ': ' . $finding['fix'], $checkup['findings']),
    ], JSON_PRETTY_PRINT | JSON_UNESCAPED_SLASHES) . "\n";
};
echo $summary($doctor("{$extension}\n"));
echo $summary($doctor(''));
$opcache = '';
foreach (opcov_opcache_ini() as $name => $value) {
    $opcache .= "$name=$value\n";
}
echo $summary($doctor("{$extension}\n{$opcache}opcache.enable_cli=1\nopcache.protect_memory=1\n"));
unlink($scan . '/opcov.ini');
rmdir($scan);
?>
--EXPECT--
{
    "exit": 0,
    "loaded": true,
    "thread_safety_matches": true,
    "self_test": "passed",
    "findings": []
}
{
    "exit": 1,
    "loaded": false,
    "thread_safety_matches": true,
    "self_test": "not_run",
    "findings": [
        "extension-not-loaded: add extension=opcov (or the full path of opcov.so) to an INI file this PHP reads; `php --ini` lists them"
    ]
}
{
    "exit": 1,
    "loaded": true,
    "thread_safety_matches": true,
    "self_test": "not_run",
    "findings": [
        "opcache-protect-memory: set opcache.protect_memory=0",
        "opcache-optimizer: set opcache.optimization_level=0"
    ]
}
