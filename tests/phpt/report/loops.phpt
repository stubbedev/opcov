--TEST--
write_report() attributes loop flags per test: zero lists the tests that ran a loop zero times, second the tests that ran it two or more times, and a do-while has zero null; outcomes carry their site
--INI--
opcov.enabled=1
opcov.collect_paths={PWD}/fixtures/loops
--FILE--
<?php
require __DIR__ . '/fixtures/report.inc';
$root = __DIR__ . '/fixtures/loops';
require "$root/sum.php";
foreach (['Tests\Empty' => [], 'Tests\One' => [1], 'Tests\Two' => [1, 2]] as $test => $values) {
    \opcov\start();
    sum($values);
    countdown(count($values));
    \opcov\stop($test);
    \opcov\reset();
}
$dir = opcov_report_dir();
\opcov\write_report("$dir/report.json", ['root' => $root]);
$report = json_decode(file_get_contents("$dir/report.json"), true);
opcov_remove_dir($dir);
echo implode(',', $report['tests']), "\n";
foreach ($report['files']['sum.php']['functions'] as $key => $function) {
    foreach ($function['loops'] as $loop) {
        echo "$key {$loop['id']} {$loop['kind']} line {$loop['line']}: zero ", json_encode($loop['zero']), ' second ', json_encode($loop['second']), "\n";
    }
    foreach ($function['outcomes'] as $outcome) {
        echo "$key {$outcome['id']} site {$outcome['site']} {$outcome['kind']}: ", json_encode($outcome['hits']), "\n";
    }
}
?>
--EXPECT--
Tests\Empty,Tests\One,Tests\Two
countdown l0 PostTest line 14: zero null second [2]
countdown o0 site 3 LoopBack: [2]
countdown o1 site 3 LoopExit: [0,1,2]
sum l0 Foreach line 5: zero [0] second [2]
sum o0 site 2 LoopEnter: [0,1,2]
sum o1 site 2 LoopSkip: []
sum o2 site 3 LoopBack: [1,2]
sum o3 site 3 LoopExit: [0,1,2]
