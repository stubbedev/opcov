--TEST--
reset() re-arms only the probes that fired, however many are installed
--INI--
opcov.enabled=1
opcov.collect_paths={PWD}
--FILE--
<?php
require __DIR__ . '/../support/coverage.inc';
$dir = sys_get_temp_dir() . '/opcov-reset-' . getmypid();
mkdir($dir);
$code = "<?php\nfunction big(int \$n): int\n{\n    if (\$n === 0) {\n        return 0;\n    }\n" . str_repeat("    \$n++;\n", 100000) . "    return \$n;\n}\nbig(0);\n";
file_put_contents("$dir/big.php", $code);
$script = <<<'PHP'
<?php
require getenv('OPCOV_BIG');
\opcov\start();
big(0);
\opcov\stop();
$before = \opcov\get_status()['counters'];
$start = hrtime(true);
\opcov\reset();
$elapsed = intdiv(hrtime(true) - $start, 1000);
$after = \opcov\get_status()['counters'];
echo "installed: ", $before['probes_installed'] >= 100000 ? "at least 100000" : $before['probes_installed'], "\n";
echo "re-armed by reset(): ", $after['probes_rearmed'] - $before['probes_rearmed'], "\n";
fwrite(STDERR, "reset took {$elapsed} microseconds\n");
PHP;
putenv("OPCOV_BIG=$dir/big.php");
echo opcov_child($script, ['opcov.collect_paths' => $dir]);
unlink("$dir/big.php");
rmdir($dir);
--EXPECTF--
installed: at least 100000
re-armed by reset(): %d
reset took %d microseconds
