--TEST--
write_report() refuses with CannotCollectException while collection is off
--INI--
opcov.enabled=1
--FILE--
<?php
try {
    \opcov\write_report(sys_get_temp_dir() . '/opcov-refused.json');
} catch (\opcov\CannotCollectException $e) {
    echo $e->getMessage(), "\n";
}
--EXPECT--
opcov cannot collect coverage: opcov.collect_paths is not set
