--TEST--
Two collections touching the same line both report it, because reset() re-arms the probes that fired
--INI--
opcov.enabled=1
opcov.collect_paths={PWD}/fixtures
--FILE--
<?php
require __DIR__ . "/fixtures/work.php";
$file = __DIR__ . "/fixtures/work.php";
foreach (["first", "second"] as $test) {
    \opcov\start();
    work(2);
    echo $test, ": ", implode(",", array_keys(\opcov\stop()[$file]["lines"])), "\n";
    \opcov\reset();
}
--EXPECT--
first: 4,5,6,8
second: 4,5,6,8
