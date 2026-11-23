--TEST--
After pcntl_fork() parent and child each report their own hits independently
--INI--
extension={ENV:OPCOV_PCNTL_EXTENSION}
opcov.enabled=1
opcov.collect_paths={PWD}/fixtures
--FILE--
<?php
$file = __DIR__ . '/fixtures/branches.php';
require $file;
\opcov\start();
branch(0);
$pid = pcntl_fork();
if ($pid === 0) {
    branch(1);
    echo "child: ", implode(',', array_keys(\opcov\stop()[$file]['lines'])), "\n";
    exit(0);
}
pcntl_waitpid($pid, $status);
branch(2);
echo "parent: ", implode(',', array_keys(\opcov\stop()[$file]['lines'])), "\n";
--EXPECT--
child: 4,5,7,8
parent: 4,5,7,10
