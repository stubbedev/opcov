--TEST--
eval() code is measured when its host file is selected and reported under <file>(<line>) : eval()'d code #<first 16 hex digits of the source's sha256>, and opcov\get_debug_info() keys its functions as opcov\executable() does
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
foreach (\opcov\executable(\opcov\get_status()["selected_files"]) as $file => $model) {
    $missing = array_diff(array_keys($model["functions"]), array_keys(\opcov\get_debug_info($file)["functions"]));
    echo str_replace(__DIR__ . "/", "", $file), ": ", $missing === [] ? "debug info keys match" : "debug info lacks " . implode(", ", $missing), "\n";
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
fixtures/app/evaluates.php(4) : eval()'d code #085f279b7135a860: 2
fixtures/app/evaluates.php: debug info keys match
fixtures/app/evaluates.php(4) : eval()'d code #085f279b7135a860: debug info keys match
eval.php: not in opcov.collect_paths
eval.php(5) : eval()'d code: not in opcov.collect_paths
