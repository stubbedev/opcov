--TEST--
PHP 8.6 compiles the closure of a partial function application at run time; it is adopted on its first probe hit
--SKIPIF--
<?php
if (PHP_VERSION_ID < 80600) {
    die('skip partial function application needs PHP 8.6');
}
?>
--INI--
opcov.enabled=1
opcov.collect_paths={PWD}/fixtures
--FILE--
<?php
$file = __DIR__ . '/fixtures/partial.php';
\opcov\start();
require $file;
echo implode(',', array_keys(\opcov\stop()[$file]['lines'])), "\n";
\opcov\reset();
\opcov\start();
echo $greet('night'), "\n";
echo implode(',', array_keys(\opcov\stop()[$file]['lines'])), "\n";
--EXPECT--
good morning world
4,7,8,9
good night world
4,7
