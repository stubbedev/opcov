--TEST--
An invalid opcov.exclude_pattern turns collection off with the pattern in the message and prints nothing at startup
--INI--
opcov.enabled=1
opcov.collect_paths={PWD}/fixtures
opcov.exclude_pattern=~unclosed
--FILE--
<?php
require __DIR__ . "/fixtures/report.inc";
report();
try {
    \opcov\ensure_can_collect();
} catch (opcov\CannotCollectException $e) {
    echo $e->getMessage(), "\n";
}
--EXPECT--
off: opcov.exclude_pattern is not a valid PCRE pattern with delimiters: ~unclosed
opcov cannot collect coverage: opcov.exclude_pattern is not a valid PCRE pattern with delimiters: ~unclosed
