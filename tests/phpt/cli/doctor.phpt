--TEST--
opcov doctor runs the probe self-test in a child PHP that reads the user's INI files: it passes with opcov loaded, names an unloaded extension, and lists every OPcache conflict with its fix and troubleshooting link
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
preg_match('~extension=["\']?([^"\' ]*opcov[^"\' ]*)~', getenv('TEST_PHP_EXTRA_ARGS'), $match);
$scan = sys_get_temp_dir() . '/opcov-doctor-ini-' . getmypid();
mkdir($scan);
$doctor = static function (string $ini) use ($scan): string {
    file_put_contents($scan . '/opcov.ini', $ini);
    $command = 'PHP_INI_SCAN_DIR=' . escapeshellarg($scan) . ' PHPRC= '
        . escapeshellarg(getenv('OPCOV_CLI')) . ' doctor --php ' . escapeshellarg(PHP_BINARY) . ' --format json; echo "exit $?"';
    return opcov_child('<?php echo shell_exec(' . var_export($command, true) . ');');
};
$summary = static function (string $output): string {
    [$json, $exit] = explode("\nexit ", trim($output));
    $checkup = json_decode($json, true);
    return json_encode([
        'exit' => (int) $exit,
        'loaded' => $checkup['runtime'] !== null,
        'self_test' => $checkup['self_test'],
        'findings' => array_map(static fn (array $finding): string => $finding['code'] . ': ' . $finding['fix'], $checkup['findings']),
    ], JSON_PRETTY_PRINT | JSON_UNESCAPED_SLASHES) . "\n";
};
echo $summary($doctor("extension={$match[1]}\n"));
echo $summary($doctor(''));
$opcache = '';
foreach (opcov_opcache_ini() as $name => $value) {
    $opcache .= "$name=$value\n";
}
echo $summary($doctor("extension={$match[1]}\n{$opcache}opcache.enable_cli=1\nopcache.protect_memory=1\n"));
unlink($scan . '/opcov.ini');
rmdir($scan);
?>
--EXPECT--
{
    "exit": 0,
    "loaded": true,
    "self_test": "passed",
    "findings": []
}
{
    "exit": 1,
    "loaded": false,
    "self_test": "not_run",
    "findings": [
        "extension-not-loaded: add extension=opcov (or the full path of opcov.so) to an INI file this PHP reads; `php --ini` lists them"
    ]
}
{
    "exit": 1,
    "loaded": true,
    "self_test": "not_run",
    "findings": [
        "opcache-protect-memory: set opcache.protect_memory=0",
        "opcache-optimizer: set opcache.optimization_level=0"
    ]
}
