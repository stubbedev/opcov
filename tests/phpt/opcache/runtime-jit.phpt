--TEST--
opcache.jit=off (which the INI parser stores as an empty string) leaves the JIT switchable at run time; a script that turns it on with ini_set() makes opcov refuse from then on
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
echo opcov_child(<<<PHP
<?php
require '$support';
echo opcov_refusal();
var_dump(ini_set('opcache.jit', 'tracing'));
echo opcov_refusal();
PHP, ['opcache.enable_cli' => '1', 'opcache.protect_memory' => '0', 'opcache.optimization_level' => '0', 'opcache.jit' => 'off', 'opcache.jit_buffer_size' => '16M'] + opcov_opcache_ini());
?>
--EXPECT--
can collect
string(0) ""
opcov cannot collect coverage: opcache.jit=tracing with opcache.jit_buffer_size=16M turns the JIT on, and JIT-compiled code never reads the opcode handlers that probes replace; set opcache.jit=disable [opcache-jit]
