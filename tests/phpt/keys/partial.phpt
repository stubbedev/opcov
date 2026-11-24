--TEST--
The closure of a partial function application, which PHP 8.6 creates at run time, ranks after the closures compiled with the file that share its line span, so it never takes over their key
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
require $file;
\opcov\start();
echo $hello('world'), "\n";
\opcov\stop();
$armed = [];
foreach (\opcov\get_debug_info($file)['probes'] as $probe) {
    $armed[$probe['function']][] = $probe['armed'] === [] ? 'ran' : 'idle';
}
foreach (array_keys(\opcov\executable([$file])[$file]['functions']) as $key) {
    $name = str_replace($file, 'partial.php', $key);
    if (str_starts_with($name, '{main}')) {
        echo $name, "\n";
        continue;
    }
    $state = isset($armed[$key]) ? implode(',', array_unique($armed[$key])) : 'no probes';
    echo $name, ": $state\n";
}
--EXPECT--
hello world
{main}@partial.php
join_words: ran
{closure}@partial.php:3-3: idle
{closure}@partial.php:3-3#1: ran
