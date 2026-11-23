--TEST--
Nested start(), stop() without start() and reset() while collecting raise LogicException; ensure_can_collect() after a collection that selected no file throws
--INI--
opcov.enabled=1
opcov.collect_paths={PWD}/fixtures
--FILE--
<?php
foreach ([
    "stop without start" => static fn () => \opcov\stop(),
    "nested start" => static function (): void { \opcov\start(); \opcov\start(); },
    "reset while collecting" => static fn () => \opcov\reset(),
] as $case => $call) {
    try {
        $call();
        echo "$case: no error\n";
    } catch (LogicException $e) {
        echo "$case: ", get_class($e), ": ", $e->getMessage(), "\n";
    }
}
var_dump(\opcov\get_status()["collecting"]);
\opcov\stop();
try {
    \opcov\ensure_can_collect();
} catch (opcov\CannotCollectException $e) {
    echo get_class($e), ": ", $e->getMessage(), "\n";
}
--EXPECT--
stop without start: LogicException: opcov\stop() was called without opcov\start()
nested start: LogicException: opcov\start() was called while collecting; call opcov\stop() first
reset while collecting: LogicException: opcov\reset() was called while collecting; call opcov\stop() first
bool(true)
opcov\CannotCollectException: opcov cannot collect coverage: opcov.collect_paths selected none of the 1 files compiled during this run
