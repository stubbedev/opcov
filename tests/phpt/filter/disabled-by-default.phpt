--TEST--
Loading the extension alone changes nothing: without opcov.enabled=1 collection is refused with the reason
--INI--

--FILE--
<?php
try {
    \opcov\start();
} catch (opcov\CannotCollectException $e) {
    echo get_class($e), " extends ", get_parent_class($e), ": ", $e->getMessage(), "\n";
}
var_dump(\opcov\get_status()["enabled"]);
--EXPECT--
opcov\CannotCollectException extends RuntimeException: opcov cannot collect coverage: opcov.enabled was 0 when PHP started; set opcov.enabled=1 in php.ini or with -d (see https://github.com/stubbedev/opcov/blob/main/docs/troubleshooting.md#not-enabled-at-startup)
bool(false)
