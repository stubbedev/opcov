--TEST--
A closure in a constant expression is compiled with its file but only reaches opcov when it first runs; it ranks after the closures compiled with the file that share its line span
--SKIPIF--
<?php
if (PHP_VERSION_ID < 80500) {
    die('skip closures in constant expressions need PHP 8.5');
}
?>
--INI--
opcov.enabled=1
opcov.collect_paths={PWD}/fixtures
--FILE--
<?php
$file = __DIR__ . '/fixtures/deferred.php';
require $file;
\opcov\start();
[$deferred, $compiled] = defaults();
echo $deferred(), "\n";
\opcov\stop();
$armed = [];
foreach (\opcov\get_debug_info($file)['probes'] as $probe) {
    $armed[$probe['function']][] = $probe['armed'] === [] ? 'ran' : 'idle';
}
foreach (array_keys(\opcov\executable([$file])[$file]['functions']) as $key) {
    $state = isset($armed[$key]) ? implode(',', array_unique($armed[$key])) : 'no probes';
    echo str_replace($file, 'deferred.php', $key), ": $state\n";
}
--EXPECT--
deferred
{main}@deferred.php: no probes
defaults: ran
{closure}@deferred.php:2-2: idle
{closure}@deferred.php:2-2#1: ran
