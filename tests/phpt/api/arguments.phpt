--TEST--
The collection functions validate their arguments
--INI--
opcov.enabled=1
opcov.collect_paths={PWD}/fixtures
--FILE--
<?php
foreach ([
    static fn () => \opcov\start(1),
    static fn () => \opcov\executable("x"),
    static fn () => \opcov\get_debug_info("a\0b"),
] as $call) {
    try {
        $call();
    } catch (Error $e) {
        echo get_class($e), ": ", $e->getMessage(), "\n";
    }
}
--EXPECT--
ArgumentCountError: opcov\start() expects exactly 0 arguments, 1 given
TypeError: opcov\executable(): Argument #1 ($files) must be of type array, string given
ValueError: opcov\get_debug_info(): Argument #1 ($file) must not contain any null bytes
