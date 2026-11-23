--TEST--
The pipe operator (PHP 8.5): every stage of a pipe chain is measured, and a closure stage reports its own outcomes
--SKIPIF--
<?php if (PHP_VERSION_ID < 80500) die('skip the pipe operator needs PHP 8.5'); ?>
--INI--
opcov.enabled=1
opcov.collect_paths={PWD}/fixtures
--FILE--
<?php
require __DIR__ . '/../support/coverage.inc';
$file = __DIR__ . '/fixtures/pipe.php';
require $file;
echo opcov_branches($file, static fn () => print(piped(' a ') . "\n"));
echo opcov_branches($file, static fn () => print(piped('  ') . "\n"));
echo opcov_lines($file, static fn () => piped('b'));
?>
--EXPECT--
A
  {closure}@pipe.php:6-6: IfFalse@6
empty
  {closure}@pipe.php:6-6: IfTrue@6
pipe.php
  covered: 4,5,6
  uncovered:
