--TEST--
stop() returns, per file, the lines and, per function, the outcome ids and loop flags hit since start(); reset() re-arms outcome and loop probes so two collections both see them; outcomes hit while not collecting are strays that start() re-arms
--INI--
opcov.enabled=1
opcov.collect_paths={PWD}/fixtures
--FILE--
<?php
require __DIR__ . '/fixtures/work.php';
$file = __DIR__ . '/fixtures/work.php';
work(2);
foreach (['first', 'second'] as $test) {
    \opcov\start();
    work(0);
    work(2);
    $hits = \opcov\stop();
    \opcov\reset();
    echo $test, ': ', json_encode($hits[$file]['functions']), "\n";
}
\opcov\start();
work(1);
echo 'one iteration: ', json_encode(\opcov\stop()[$file]['functions']['work']), "\n";
\opcov\reset();
\opcov\start();
echo 'nothing: ', json_encode(\opcov\stop()), "\n";
?>
--EXPECT--
first: {"work":{"outcomes":{"o0":true,"o1":true},"loops":{"l0":{"zero":true,"second":true}}}}
second: {"work":{"outcomes":{"o0":true,"o1":true},"loops":{"l0":{"zero":true,"second":true}}}}
one iteration: {"outcomes":{"o0":true,"o1":true},"loops":[]}
nothing: []
