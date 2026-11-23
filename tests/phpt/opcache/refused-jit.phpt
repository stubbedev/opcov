--TEST--
Every JIT mode under the CLI is refused before any probe is installed, with a message that names the setting, the value to use and the troubleshooting entry; a JIT together with the optimizer lists both conflicts
--SKIPIF--
<?php
require __DIR__ . '/../support/coverage.inc';
if (opcov_opcache_ini() === null) {
    die('skip OPcache is neither built in nor named by OPCOV_OPCACHE_EXTENSION');
}
if (!opcov_jit_built(opcov_opcache_ini())) {
    die('skip this OPcache is built without the JIT');
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
$base = ['opcache.enable_cli' => '1', 'opcache.protect_memory' => '0', 'opcache.optimization_level' => '0', 'opcache.jit' => 'disable'] + opcov_opcache_ini();
$cases = [
    'tracing JIT' => ['opcache.jit' => 'tracing', 'opcache.jit_buffer_size' => '16M'],
    'function JIT' => ['opcache.jit' => 'function', 'opcache.jit_buffer_size' => '16M'],
    'numeric JIT mode' => ['opcache.jit' => '1255', 'opcache.jit_buffer_size' => '16M'],
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
?>
--EXPECTF--
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
== optimizer and JIT
opcov cannot collect coverage: opcache.jit=1 with opcache.jit_buffer_size=16M turns the JIT on, and JIT-compiled code never reads the opcode handlers that probes replace; set opcache.jit=disable [opcache-jit]; opcache.optimization_level=0x7FFEBFFF lets the OPcache optimizer remove and merge code, so lines and outcomes disappear from the report; set opcache.optimization_level=0 [opcache-optimizer]
start() refused
{"off_code":"opcache-jit","conflicts":["opcache-jit","opcache-optimizer"],"probes_installed":0}
