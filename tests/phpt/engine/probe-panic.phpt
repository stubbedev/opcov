--TEST--
A panic injected into the probe callback keeps PHP running, restores every probed opcode's handler and reports that coverage could not be collected
--INI--
opcov.enabled=1
opcov.collect_paths={PWD}/fixtures
--FILE--
<?php
require __DIR__ . '/fixtures/work.php';
\opcov\start();
\opcov\selftest_panic_in_probe();
var_dump(work(3));
var_dump(work(4));
try {
    \opcov\stop();
} catch (opcov\CannotCollectException $e) {
    echo $e->getMessage(), "\n";
}
$armed = array_merge(...array_column(\opcov\get_debug_info(__DIR__ . '/fixtures/work.php')['probes'], 'armed'));
echo count($armed), " probes still armed\n";
var_dump(\opcov\get_status()['collecting']);
\opcov\reset();
var_dump(work(2));
--EXPECTF--
%Adeliberate panic injected into the probe callback
%A
