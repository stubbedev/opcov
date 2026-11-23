--TEST--
A fatal error (redeclaring a function at run time, which does not depend on the Zend allocator) inside a measured branch while collecting: the engine bails out past the probes, yet shutdown functions find opcov consistent, stop() attributes the hits made up to the error and write_report() writes the report
--INI--
opcov.enabled=1
opcov.collect_paths={PWD}/fixtures
fatal_error_backtraces=0
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
Fatal error: Cannot redeclare %s in %sabort.php on line 10
collecting at shutdown: true
tests: ["Interrupted::test"]
lines: 4,8,9
