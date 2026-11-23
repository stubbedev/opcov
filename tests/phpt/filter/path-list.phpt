--TEST--
opcov.collect_paths takes a colon-separated list of files and directories; exclude_paths removes entries
--INI--
opcov.enabled=1
opcov.collect_paths={PWD}/fixtures/app:{PWD}/fixtures/application/main.php
opcov.exclude_paths={PWD}/fixtures/app/helper.php
--FILE--
<?php
require __DIR__ . "/fixtures/report.inc";
require __DIR__ . "/fixtures/app/main.php";
require __DIR__ . "/fixtures/app/helper.php";
require __DIR__ . "/fixtures/application/main.php";
require __DIR__ . "/fixtures/application/switch.php";
report();
--EXPECT--
off: null
selected: app/main.php
selected: application/main.php
skipped: app/helper.php: excluded by opcov.exclude_paths entry app/helper.php
skipped: application/switch.php: not in opcov.collect_paths
skipped: report.inc: not in opcov.collect_paths
