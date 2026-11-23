--TEST--
Lines that ran while collection was off are re-armed by start(), so the next collection sees them
--INI--
opcov.enabled=1
opcov.collect_paths={PWD}/fixtures
--FILE--
<?php
require __DIR__ . "/fixtures/work.php";
$file = __DIR__ . "/fixtures/work.php";
work(1);
\opcov\start();
work(1);
echo implode(",", array_keys(\opcov\stop()[$file]["lines"])), "\n";
--EXPECT--
4,5,6,8
