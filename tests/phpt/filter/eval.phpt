--TEST--
eval() code is measured when its host file is selected and reported under <file>(<line>) : eval()'d code
--INI--
opcov.enabled=1
opcov.collect_paths={PWD}/fixtures/app
--FILE--
<?php
require __DIR__ . "/fixtures/app/evaluates.php";
\opcov\start();
var_dump(run_eval());
var_dump(eval("return 7;"));
foreach (\opcov\stop() as $file => $hits) {
    echo str_replace(__DIR__ . "/", "", $file), ": ", implode(",", array_keys($hits["lines"])), "\n";
}
foreach (\opcov\get_status()["skipped_files"] as $file => $reason) {
    if (str_contains($file, "eval")) {
        echo basename($file), ": $reason\n";
    }
}
--EXPECT--
int(3)
int(7)
fixtures/app/evaluates.php: 4
fixtures/app/evaluates.php(4) : eval()'d code: 2
eval.php: not in opcov.collect_paths
eval.php(5) : eval()'d code: not in opcov.collect_paths
