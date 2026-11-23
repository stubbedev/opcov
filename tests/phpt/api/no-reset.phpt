--TEST--
Without reset() a fired probe stays fired, so a second collection does not see the line again
--INI--
opcov.enabled=1
opcov.collect_paths={PWD}/fixtures
--FILE--
<?php
require __DIR__ . "/fixtures/work.php";
$file = __DIR__ . "/fixtures/work.php";
\opcov\start();
work(1);
echo implode(",", array_keys(\opcov\stop()[$file]["lines"] ?? [])), "\n";
\opcov\start();
work(1);
var_dump(\opcov\stop());
\opcov\reset();
\opcov\start();
work(1);
echo implode(",", array_keys(\opcov\stop()[$file]["lines"] ?? [])), "\n";
--EXPECT--
4,5,6,8
array(0) {
}
4,5,6,8
