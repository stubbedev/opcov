--TEST--
A closure among the arguments of an unqualified frameless call in a namespace, which PHP compiles twice (namespaced call and global fallback), is one function whose hits are the union of both copies
--INI--
opcov.enabled=1
opcov.collect_paths={PWD}/fixtures
--FILE--
<?php
$file = __DIR__ . '/fixtures/frameless.php';
require $file;
\opcov\start();
echo \N\f('ab'), "\n";
$hits = \opcov\stop();
$model = \opcov\executable([$file])[$file];
foreach ($model['functions'] as $key => $function) {
    $key = str_replace($file, 'frameless.php', $key);
    $taken = array_keys($hits[$file]['functions'][str_replace('frameless.php', $file, $key)]['outcomes'] ?? []);
    echo $key, ': outcomes ', implode(',', array_keys($function['outcomes'])), '; taken ', implode(',', $taken), "\n";
}
echo 'lines: ', implode(',', array_keys($model['lines'])), "\n";
echo 'hit:   ', implode(',', array_keys($hits[$file]['lines'])), "\n";
--EXPECT--
xy
{main}@frameless.php: outcomes ; taken 
N\f: outcomes 0,1; taken o1
{closure}@frameless.php:4-9: outcomes 0,1; taken o0,o1
lines: 4,5,6,8,9
hit:   4,5,6,8,9
