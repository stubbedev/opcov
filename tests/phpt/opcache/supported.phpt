--TEST--
OPcache under the CLI with the optimizer off and the JIT disabled is supported: probes are installed into the cached code, a file served again from the cache shares its unit, and get_status() reports OPcache as active without conflicts
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
$included = __DIR__ . '/fixtures/included.php';
echo opcov_child(<<<PHP
<?php
require '$support';
echo opcov_refusal();
\$status = \opcov\get_status();
echo json_encode(['active' => \$status['opcache']['active'], 'conflicts' => \$status['conflicts']]), "\n";
require '$branch';
echo opcov_lines('$branch', static fn () => pick(1));
echo opcov_lines('$branch', static fn () => pick(-1));
echo opcov_lines('$included', static function (): void { include '$included'; });
echo opcov_lines('$included', static function (): void { \$counter = 1; include '$included'; });
echo "cached: ", json_encode(opcache_is_script_cached('$included')), "\n";
echo "units: ", \opcov\get_debug_info('$included')['units'], "\n";
PHP, ['opcache.enable_cli' => '1', 'opcache.protect_memory' => '0', 'opcache.optimization_level' => '0', 'opcache.jit' => 'disable'] + opcov_opcache_ini());
?>
--EXPECT--
can collect
{"active":true,"conflicts":[]}
branch.php
  covered: 4,5
  uncovered: 7,8
branch.php
  covered: 4,7,8
  uncovered: 5
included.php
  covered: 2,3,6
  uncovered: 4
again
included.php
  covered: 2,3,4,6
  uncovered: 
cached: true
units: 1
