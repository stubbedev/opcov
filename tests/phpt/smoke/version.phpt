--TEST--
opcov\version() returns the crate version
--FILE--
<?php
preg_match('/^\[workspace\.package\]\nversion = "([^"]+)"$/m', file_get_contents(__DIR__ . '/../../../Cargo.toml'), $manifest);
var_dump(\opcov\version() === $manifest[1]);
var_dump(\opcov\version() === phpversion('opcov'));
--EXPECT--
bool(true)
bool(true)
