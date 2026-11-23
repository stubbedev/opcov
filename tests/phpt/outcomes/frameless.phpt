--TEST--
The two copies of a namespaced frameless call's arguments count as one: the outcomes and lines of the fallback copy (the namespaced call) are those of the frameless copy, a line only the fallback copy has is not executable, and whichever copy runs records the shared outcomes
--INI--
opcov.enabled=1
opcov.collect_paths={PWD}/fixtures/frameless
--FILE--
<?php
$file = __DIR__ . '/fixtures/frameless/calls.php';
require $file;
$report = static function (array $hits) use ($file): void {
    $model = \opcov\executable([$file])[$file];
    foreach ($model['functions'] as $key => $function) {
        $outcomes = [];
        foreach ($function['outcomes'] as $id => $outcome) {
            $outcomes[] = "$id:{$outcome['kind']}@{$outcome['site']}";
        }
        $taken = array_keys($hits[$file]['functions'][$key]['outcomes'] ?? []);
        echo str_replace($file, 'calls.php', $key), ': ', implode(' ', $outcomes), '; taken ', implode(',', $taken), "\n";
    }
    echo 'lines: ', implode(',', array_keys($model['lines'])), "\n";
    echo 'hit:   ', implode(',', array_keys($hits[$file]['lines'] ?? [])), "\n";
};
\opcov\start();
var_dump(\N\f(null), \N\f(5), \N\g('xa'));
$report(\opcov\stop());
\opcov\reset();
require __DIR__ . '/fixtures/frameless/shadow.php';
\opcov\start();
var_dump(\N\k(null), \N\k(5));
$report(\opcov\stop());
--EXPECT--
int(2)
int(5)
bool(false)
{main}@calls.php: ; taken 
N\f: 0:Engine@1 1:Engine@1 2:CoalesceFallback@9 3:CoalesceValue@9; taken o1,o2,o3
N\g: 0:Engine@1 1:Engine@1; taken o1
N\k: 0:Engine@1 1:Engine@1 2:CoalesceFallback@9 3:CoalesceValue@9; taken 
lines: 3,4,6,7
hit:   3,4,6
int(20)
int(50)
{main}@calls.php: ; taken 
N\f: 0:Engine@1 1:Engine@1 2:CoalesceFallback@9 3:CoalesceValue@9; taken 
N\g: 0:Engine@1 1:Engine@1; taken 
N\k: 0:Engine@1 1:Engine@1 2:CoalesceFallback@9 3:CoalesceValue@9; taken o0,o2,o3
lines: 3,4,6,7
hit:   7
