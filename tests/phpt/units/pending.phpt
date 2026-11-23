--TEST--
A unit whose op array is destroyed while it has pending hits lives until the hits are reported, then it is freed
--INI--
opcov.enabled=1
opcov.collect_paths={PWD}/fixtures
--FILE--
<?php
$file = __DIR__ . '/fixtures/closure.php';
\opcov\start();
$closure = require $file;
var_dump($closure(1));
$live = \opcov\get_status()['counters']['units'];
unset($closure);
echo "live units after destroying the closure: ", $live - \opcov\get_status()['counters']['units'], " fewer\n";
echo implode(',', array_keys(\opcov\stop()[$file]['lines'])), "\n";
\opcov\reset();
\opcov\start();
$again = require $file;
var_dump($again(2));
echo implode(',', array_keys(\opcov\stop()[$file]['lines'])), "\n";
--EXPECT--
int(2)
live units after destroying the closure: 1 fewer
2,3,4
int(3)
2,3,4
