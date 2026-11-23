--TEST--
A fatal error (memory exhaustion) inside a measured branch while collecting: the engine bails out past the probes, yet shutdown functions find opcov consistent, stop() attributes the hits made up to the error and write_report() writes the report
--INI--
opcov.enabled=1
opcov.collect_paths={PWD}/fixtures
memory_limit=32M
--FILE--
<?php
require __DIR__ . '/fixtures/report.inc';
$file = __DIR__ . '/fixtures/abort.php';
require $file;
register_shutdown_function(static fn () => opcov_finish_interrupted($file, 'Interrupted::test'));
\opcov\start();
abort('fatal');
?>
--EXPECTF--
Fatal error: Allowed memory size of 33554432 bytes exhausted (tried to allocate %d bytes) in %sabort.php on line 9
collecting at shutdown: true
tests: ["Interrupted::test"]
lines: 4,8,9
