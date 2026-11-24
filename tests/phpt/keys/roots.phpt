--TEST--
Closures that share a line span across enclosing declarations rank by the key of the declaration the engine compiles them under, then by compilation order
--INI--
opcov.enabled=1
opcov.collect_paths={PWD}/fixtures
--FILE--
<?php
$file = __DIR__ . '/fixtures/roots.php';
\opcov\start();
require $file;
\opcov\stop();
\opcov\reset();
\opcov\start();
alpha()();
(new Alpha())->m()();
$main();
host()();
conditional()();
blocky()();
$anonymous->m()();
(new Hooked())->p;
$destructure();
$nested()[1]()();
\opcov\stop();
$armed = [];
foreach (\opcov\get_debug_info($file)['probes'] as $probe) {
    $armed[$probe['function']][] = $probe['armed'] === [] ? 'ran' : 'idle';
}
$keys = array_keys(\opcov\executable([$file])[$file]['functions']);
foreach ($keys as $key) {
    if (!str_starts_with($key, '{closure}')) {
        continue;
    }
    $state = isset($armed[$key]) ? implode(',', array_unique($armed[$key])) : 'no probes';
    echo str_replace($file, 'roots.php', $key), ": $state\n";
}
--EXPECT--
{closure}@roots.php:2-2: ran
{closure}@roots.php:2-2#1: idle
{closure}@roots.php:3-3: ran
{closure}@roots.php:3-3#1: idle
{closure}@roots.php:4-4: ran
{closure}@roots.php:4-4#1: idle
{closure}@roots.php:5-5: idle
{closure}@roots.php:5-5#1: idle
{closure}@roots.php:5-5#2: ran
{closure}@roots.php:6-6: ran
{closure}@roots.php:6-6#1: idle
{closure}@roots.php:7-7: idle
{closure}@roots.php:7-7#1: ran
{closure}@roots.php:8-8: idle
{closure}@roots.php:8-8#1: ran
{closure}@roots.php:9-9: idle
{closure}@roots.php:9-9#1: ran
{closure}@roots.php:10-10: ran
{closure}@roots.php:10-10#1: idle
{closure}@roots.php:10-10#2: ran
{closure}@roots.php:11-11: ran
{closure}@roots.php:11-11#1: idle
{closure}@roots.php:11-11#2: ran
{closure}@roots.php:11-11#3: ran
