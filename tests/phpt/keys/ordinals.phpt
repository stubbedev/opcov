--TEST--
Closures, arrow functions and anonymous classes that share a line span get distinct keys, ranked by source position
--INI--
opcov.enabled=1
opcov.collect_paths={PWD}/fixtures
--FILE--
<?php
$file = __DIR__ . '/fixtures/ordinals.php';
require $file;
\opcov\start();
$pair[1]();
$second();
$nested();
$mixed[1]();
$classes[1]->run();
$outer->make()->run();
holder()[1]();
\opcov\stop();
$armed = [];
foreach (\opcov\get_debug_info($file)['probes'] as $probe) {
    $armed[$probe['function']][] = $probe['armed'] === [] ? 'ran' : 'idle';
}
$keys = array_keys(\opcov\executable([$file])[$file]['functions']);
foreach ($keys as $key) {
    $state = isset($armed[$key]) ? implode(',', array_unique($armed[$key])) : 'no probes';
    echo str_replace($file, 'ordinals.php', $key), ": $state\n";
}
--EXPECT--
{main}@ordinals.php: no probes
holder: ran
class@anonymous@ordinals.php:6-6::run: idle
class@anonymous@ordinals.php:6-6#1::run: ran
class@anonymous@ordinals.php:7-7::make: ran
class@anonymous@ordinals.php:7-7#1::run: ran
{closure}@ordinals.php:2-2: idle
{closure}@ordinals.php:2-2#1: ran
{closure}@ordinals.php:3-3: idle
{closure}@ordinals.php:3-3#1: ran
{closure}@ordinals.php:4-4: ran
{closure}@ordinals.php:4-4#1: idle
{closure}@ordinals.php:5-5: idle
{closure}@ordinals.php:5-5#1: ran
{closure}@ordinals.php:8-8: idle
{closure}@ordinals.php:8-8#1: ran
