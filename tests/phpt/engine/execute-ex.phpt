--TEST--
With an extension that replaces zend_execute_ex (PCOV), a probe that already fired comes back through zend_vm_call_opcode_handler() and is a no-op
--INI--
extension={ENV:OPCOV_PCOV_EXTENSION}
pcov.enabled=1
opcov.enabled=1
opcov.collect_paths={PWD}/fixtures
--FILE--
<?php
require __DIR__ . '/fixtures/work.php';
\opcov\start();
work(3);
$fired = \opcov\get_status()['counters']['probes_fired'];
work(3);
echo "fired again: ", \opcov\get_status()['counters']['probes_fired'] - $fired, "\n";
echo implode(',', array_keys(\opcov\stop()[__DIR__ . '/fixtures/work.php']['lines'])), "\n";
\opcov\reset();
\opcov\start();
work(1);
echo implode(',', array_keys(\opcov\stop()[__DIR__ . '/fixtures/work.php']['lines'])), "\n";
--EXPECT--
fired again: 0
4,5,6,8
4,5,6,8
