--TEST--
Executable lines and probes follow PCOV's rules: one probe per line, ignored opcodes, NEW with its constructor once (PHP 8.6 sends the constant argument on line 20 with the ignored SEND_VAL, earlier versions with SEND_VAL_EX)
--INI--
opcov.enabled=1
opcov.collect_paths={PWD}/fixtures
--FILE--
<?php
require __DIR__ . '/../support/coverage.inc';
echo opcov_lines(__DIR__ . "/fixtures/basic.php", static function (): void {
    require __DIR__ . "/fixtures/basic.php";
});
--EXPECTF--
basic.php
  covered: 4,7,9,10,18,19,%r(20,)?%r22
  uncovered: 5,8,15,16
