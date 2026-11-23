--TEST--
opcov\version() rejects arguments
--FILE--
<?php
try {
    \opcov\version(1);
} catch (ArgumentCountError $e) {
    echo $e->getMessage(), "\n";
}
--EXPECT--
opcov\version() expects exactly 0 arguments, 1 given
