--TEST--
exit() inside a measured branch while collecting: shutdown functions still run, stop() attributes the hits made up to the exit to the interrupted test, and write_report() writes a complete report
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
abort('exit');
?>
--EXPECTF--
exiting
collecting at shutdown: true
tests: ["Interrupted::test"]
lines: 4,5,6
