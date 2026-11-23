--TEST--
opcov.exclude_pattern is a PCRE with delimiters, matched against the resolved path with PHP's own PCRE
--INI--
opcov.enabled=1
opcov.collect_paths={PWD}/fixtures
opcov.exclude_pattern=~/app/h[a-z]+\.php$~
--FILE--
<?php
require __DIR__ . "/fixtures/report.inc";
require __DIR__ . "/fixtures/app/main.php";
require __DIR__ . "/fixtures/app/helper.php";
report();
--EXPECT--
off: null
selected: report.inc
selected: app/main.php
skipped: app/helper.php: matched by opcov.exclude_pattern
