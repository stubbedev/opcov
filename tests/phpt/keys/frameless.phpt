--TEST--
A closure among the arguments of an unqualified frameless call in a namespace, which PHP compiles twice (namespaced call and global fallback), is one function whose hits are the union of both copies, also when the copies differ only in the bits of unused operands (a method call on $this), and closures nested in the copies are one function too
--INI--
opcov.enabled=1
opcov.collect_paths={PWD}/fixtures
--FILE--
<?php
$file = __DIR__ . '/fixtures/frameless.php';
require $file;
\opcov\start();
echo \N\f('ab'), "\n";
echo (new \N\Joiner())->join(['a', 'b']), "\n";
echo \N\g(['ab']), "\n";
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
[a],[b]
<a><b>
{main}@frameless.php: outcomes ; taken 
N\f: outcomes 0,1; taken o1
N\g: outcomes 0,1; taken o1
N\Joiner::join: outcomes 0,1; taken o1
N\Wraps::wrap: outcomes ; taken 
{closure}@frameless.php:4-9: outcomes 0,1; taken o0,o1
{closure}@frameless.php:20-20: outcomes ; taken 
{closure}@frameless.php:24-26: outcomes ; taken 
{closure}@frameless.php:25-25: outcomes ; taken 
lines: 4,5,6,8,9,13,20,24,25,26
hit:   4,5,6,8,9,13,20,24,25,26
