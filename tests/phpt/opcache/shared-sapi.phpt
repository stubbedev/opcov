--TEST--
OPcache under a SAPI that shares compiled code between processes (CGI) is refused whatever opcache.enable_cli says; with opcache.enable=0 the same SAPI collects
--SKIPIF--
<?php
require __DIR__ . '/../support/coverage.inc';
if (opcov_opcache_ini() === null) {
    die('skip OPcache is neither built in nor named by OPCOV_OPCACHE_EXTENSION');
}
if (opcov_sibling_binary('php-cgi') === null) {
    die('skip php-cgi is not installed next to the PHP binary');
}
?>
--INI--
opcov.enabled=1
opcov.collect_paths={PWD}/fixtures
--FILE--
<?php
require __DIR__ . '/../support/coverage.inc';
$support = __DIR__ . '/../support/coverage.inc';
$script = <<<PHP
<?php
require '$support';
echo PHP_SAPI, ": ", opcov_refusal();
echo json_encode(array_column(\opcov\get_status()['conflicts'], 'code')), "\n";
PHP;
$cgi = opcov_sibling_binary('php-cgi');
echo opcov_child($script, ['opcache.enable' => '1', 'opcache.enable_cli' => '0'] + opcov_opcache_ini(), $cgi, [], ['-q']);
echo opcov_child($script, ['opcache.enable' => '0'] + opcov_opcache_ini(), $cgi, [], ['-q']);
?>
--EXPECT--
cgi-fcgi: opcov cannot collect coverage: OPcache is enabled under the cgi-fcgi SAPI, which shares the compiled code between processes, so a probe in one process would rewrite the code of all of them; set opcache.enable=0, or collect coverage under the CLI SAPI [opcache-shared-sapi]
["opcache-shared-sapi"]
cgi-fcgi: can collect
[]
