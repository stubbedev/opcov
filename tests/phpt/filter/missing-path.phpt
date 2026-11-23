--TEST--
A collect path that does not exist is refused with the path in the message
--INI--
opcov.enabled=1
opcov.collect_paths={PWD}/fixtures/does-not-exist
--FILE--
<?php
try {
    \opcov\ensure_can_collect();
} catch (opcov\CannotCollectException $e) {
    echo str_replace(__DIR__ . "/", "", $e->getMessage()), "\n";
}
--EXPECT--
opcov cannot collect coverage: opcov.collect_paths: cannot resolve fixtures/does-not-exist: No such file or directory (os error 2) (see https://github.com/stubbedev/opcov/blob/main/docs/troubleshooting.md#invalid-selection)
