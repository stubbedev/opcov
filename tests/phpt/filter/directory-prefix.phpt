--TEST--
A directory matches by whole path components, so app does not select application/
--INI--
opcov.enabled=1
opcov.collect_paths={PWD}/fixtures/app
--FILE--
<?php
require __DIR__ . "/fixtures/report.inc";
require __DIR__ . "/fixtures/app/main.php";
require __DIR__ . "/fixtures/application/main.php";
report();
--EXPECT--
off: null
selected: app/main.php
skipped: application/main.php: not in opcov.collect_paths
skipped: report.inc: not in opcov.collect_paths
