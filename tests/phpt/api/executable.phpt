--TEST--
executable() reports the lines, outcomes and loops of compiled files, in the report schema's shape; the RETURN of a declarations-only file is not executable
--INI--
opcov.enabled=1
opcov.collect_paths={PWD}/fixtures
--FILE--
<?php
require __DIR__ . "/fixtures/work.php";
$file = __DIR__ . "/fixtures/work.php";
$model = \opcov\executable([$file, __DIR__ . "/fixtures/never-compiled.php", __FILE__]);
echo json_encode(array_keys($model) === [$file]), "\n";
echo json_encode($model[$file]["lines"]), "\n";
echo json_encode(array_map(static fn ($key) => preg_replace("~@.*/~", "@", $key), array_keys($model[$file]["functions"]))), "\n";
echo json_encode($model[$file]["functions"]["work"]), "\n";
try {
    \opcov\executable([42]);
} catch (LogicException $e) {
    echo $e->getMessage(), "\n";
}
--EXPECT--
true
{"4":[],"5":[],"6":[],"8":[]}
["{main}@work.php","work"]
{"loops":[{"id":"l0","kind":"PreTest","line":5,"second":[],"site":7,"zero":[]}],"outcomes":[{"hits":[],"id":"o0","kind":"LoopBack","line":5,"site":7,"span":null},{"hits":[],"id":"o1","kind":"LoopExit","line":5,"site":7,"span":null}],"span":null}
opcov\executable() expects file paths; element 0 is not a string
