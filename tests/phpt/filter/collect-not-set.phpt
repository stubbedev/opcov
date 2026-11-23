--TEST--
With opcov.enabled=1 but no opcov.collect_paths, collection is refused
--INI--
opcov.enabled=1
--FILE--
<?php
try {
    \opcov\start();
} catch (opcov\CannotCollectException $e) {
    echo $e->getMessage(), "\n";
}
--EXPECT--
opcov cannot collect coverage: opcov.collect_paths is not set; set it to the files and directories to measure (see https://github.com/stubbedev/opcov/blob/main/docs/troubleshooting.md#collect-paths-empty)
