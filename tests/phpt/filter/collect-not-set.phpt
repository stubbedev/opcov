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
opcov cannot collect coverage: opcov.collect_paths is not set
