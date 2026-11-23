--TEST--
get_status() reports collection state, the off reason, selected and skipped files with the reason, and counters
--INI--
opcov.enabled=1
opcov.collect_paths={PWD}/fixtures
--FILE--
<?php
require __DIR__ . "/fixtures/work.php";
\opcov\start();
work(1);
$status = \opcov\get_status();
\opcov\stop();
var_dump($status["enabled"], $status["collecting"], $status["off_reason"]);
echo json_encode(array_map("basename", $status["selected_files"])), "\n";
foreach ($status["skipped_files"] as $file => $reason) {
    echo basename($file), ": $reason\n";
}
echo implode(",", array_keys($status["counters"])), "\n";
var_dump($status["counters"]["probes_installed"] > 0, $status["counters"]["probes_fired"] > 0);
--EXPECT--
bool(true)
bool(true)
NULL
["work.php"]
status.php: not in opcov.collect_paths
jump_table_files,probes_fired,probes_installed,probes_rearmed,units
bool(true)
bool(true)
