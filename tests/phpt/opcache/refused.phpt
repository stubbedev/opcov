--TEST--
Every unsupported OPcache setup under the CLI is refused before any probe is installed, with a message that names the setting, the value to use and the troubleshooting entry; several conflicts are all listed
--SKIPIF--
<?php
require __DIR__ . '/../support/coverage.inc';
if (opcov_opcache_ini() === null) {
    die('skip OPcache is neither built in nor named by OPCOV_OPCACHE_EXTENSION');
}
?>
--INI--
opcov.enabled=1
opcov.collect_paths={PWD}/fixtures
--FILE--
<?php
require __DIR__ . '/../support/coverage.inc';
$support = __DIR__ . '/../support/coverage.inc';
$branch = __DIR__ . '/fixtures/branch.php';
$cache = sys_get_temp_dir() . '/opcov-file-cache-' . getmypid();
mkdir($cache);
$base = ['opcache.enable_cli' => '1', 'opcache.protect_memory' => '0', 'opcache.optimization_level' => '0', 'opcache.jit' => 'disable'] + opcov_opcache_ini();
$cases = [
    'optimizer at its default level' => ['opcache.optimization_level' => '0x7FFEBFFF'],
    'optimizer with one pass' => ['opcache.optimization_level' => '1'],
    'tracing JIT' => ['opcache.jit' => 'tracing', 'opcache.jit_buffer_size' => '16M'],
    'function JIT' => ['opcache.jit' => 'function', 'opcache.jit_buffer_size' => '16M'],
    'numeric JIT mode' => ['opcache.jit' => '1255', 'opcache.jit_buffer_size' => '16M'],
    'protected memory' => ['opcache.protect_memory' => '1'],
    'file cache' => ['opcache.file_cache' => $cache],
    'preload' => ['opcache.preload' => __DIR__ . '/fixtures/preload.php'],
    'optimizer and JIT' => ['opcache.optimization_level' => '0x7FFEBFFF', 'opcache.jit' => 'on', 'opcache.jit_buffer_size' => '16M'],
];
foreach ($cases as $name => $settings) {
    echo "== $name\n";
    echo opcov_child(<<<PHP
<?php
require '$support';
echo opcov_refusal();
try {
    \opcov\start();
} catch (opcov\CannotCollectException) {
    echo "start() refused\n";
}
require '$branch';
pick(1);
\$status = \opcov\get_status();
echo json_encode(['off_code' => \$status['off_code'], 'conflicts' => array_column(\$status['conflicts'], 'code'), 'probes_installed' => \$status['counters']['probes_installed']]), "\n";
PHP, $settings + $base);
}
foreach (new RecursiveIteratorIterator(new RecursiveDirectoryIterator($cache, FilesystemIterator::SKIP_DOTS), RecursiveIteratorIterator::CHILD_FIRST) as $entry) {
    $entry->isDir() ? rmdir($entry->getPathname()) : unlink($entry->getPathname());
}
rmdir($cache);
?>
--EXPECTF--
== optimizer at its default level
opcov cannot collect coverage: opcache.optimization_level=0x7FFEBFFF lets the OPcache optimizer remove and merge code, so lines and outcomes disappear from the report; set opcache.optimization_level=0 [opcache-optimizer]
start() refused
{"off_code":"opcache-optimizer","conflicts":["opcache-optimizer"],"probes_installed":0}
== optimizer with one pass
opcov cannot collect coverage: opcache.optimization_level=1 lets the OPcache optimizer remove and merge code, so lines and outcomes disappear from the report; set opcache.optimization_level=0 [opcache-optimizer]
start() refused
{"off_code":"opcache-optimizer","conflicts":["opcache-optimizer"],"probes_installed":0}
== tracing JIT
opcov cannot collect coverage: opcache.jit=tracing with opcache.jit_buffer_size=16M turns the JIT on, and JIT-compiled code never reads the opcode handlers that probes replace; set opcache.jit=disable [opcache-jit]
start() refused
{"off_code":"opcache-jit","conflicts":["opcache-jit"],"probes_installed":0}
== function JIT
opcov cannot collect coverage: opcache.jit=function with opcache.jit_buffer_size=16M turns the JIT on, and JIT-compiled code never reads the opcode handlers that probes replace; set opcache.jit=disable [opcache-jit]
start() refused
{"off_code":"opcache-jit","conflicts":["opcache-jit"],"probes_installed":0}
== numeric JIT mode
opcov cannot collect coverage: opcache.jit=1255 with opcache.jit_buffer_size=16M turns the JIT on, and JIT-compiled code never reads the opcode handlers that probes replace; set opcache.jit=disable [opcache-jit]
start() refused
{"off_code":"opcache-jit","conflicts":["opcache-jit"],"probes_installed":0}
== protected memory
opcov cannot collect coverage: opcache.protect_memory=1 makes the compiled code read-only, so probes cannot be installed; set opcache.protect_memory=0 [opcache-protect-memory]
start() refused
{"off_code":"opcache-protect-memory","conflicts":["opcache-protect-memory"],"probes_installed":0}
== file cache
opcov cannot collect coverage: opcache.file_cache=%s stores the compiled code, probes included, on disk for other processes; set opcache.file_cache= (empty) and opcache.file_cache_only=0 [opcache-file-cache]
start() refused
{"off_code":"opcache-file-cache","conflicts":["opcache-file-cache"],"probes_installed":0}
== preload
opcov cannot collect coverage: opcache.preload=%spreload.php compiles code before opcov installs its hooks, so preloaded code gets no probes; set opcache.preload= (empty) [opcache-preload]
start() refused
{"off_code":"opcache-preload","conflicts":["opcache-preload"],"probes_installed":0}
== optimizer and JIT
opcov cannot collect coverage: opcache.jit=1 with opcache.jit_buffer_size=16M turns the JIT on, and JIT-compiled code never reads the opcode handlers that probes replace; set opcache.jit=disable [opcache-jit]; opcache.optimization_level=0x7FFEBFFF lets the OPcache optimizer remove and merge code, so lines and outcomes disappear from the report; set opcache.optimization_level=0 [opcache-optimizer]
start() refused
{"off_code":"opcache-jit","conflicts":["opcache-jit","opcache-optimizer"],"probes_installed":0}
