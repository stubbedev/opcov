--TEST--
Under phpdbg, which replaces zend_execute_ex and runs every opcode through zend_vm_call_opcode_handler() (ignoring opline->handler on the HYBRID VM), a probe that already fired comes back and is a no-op
--SKIPIF--
<?php
require __DIR__ . '/../support/coverage.inc';
if (opcov_sibling_binary('phpdbg') === null) {
    die('skip phpdbg is not installed next to the PHP binary');
}
?>
--INI--
opcov.enabled=1
opcov.collect_paths={PWD}/fixtures
--FILE--
<?php
require __DIR__ . '/../support/coverage.inc';
$work = __DIR__ . '/fixtures/work.php';
echo opcov_child(<<<PHP
<?php
require '$work';
echo PHP_SAPI, "\n";
\opcov\start();
work(3);
\$fired = \opcov\get_status()['counters']['probes_fired'];
work(3);
echo "fired again: ", \opcov\get_status()['counters']['probes_fired'] - \$fired, "\n";
echo implode(',', array_keys(\opcov\stop()['$work']['lines'])), "\n";
\opcov\\reset();
\opcov\start();
work(1);
echo implode(',', array_keys(\opcov\stop()['$work']['lines'])), "\n";
PHP, [], opcov_sibling_binary('phpdbg'), [], '-qrr');
?>
--EXPECT--
phpdbg
fired again: 0
4,5,6,8
4,5,6,8
