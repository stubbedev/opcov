--TEST--
Symlinks and .. segments resolve to one real path; a file included under two paths is one file
--INI--
opcov.enabled=1
opcov.collect_paths={PWD}/fixtures/app
--FILE--
<?php
require __DIR__ . "/../support/coverage.inc";
$dir = sys_get_temp_dir() . "/opcov-symlink-" . getmypid();
mkdir($dir);
symlink(__DIR__ . "/fixtures/app", "$dir/linked");
$script = "<?php require \"" . __DIR__ . "/fixtures/report.inc\";
\\opcov\\start();
require \"$dir/linked/main.php\";
require_once \"" . __DIR__ . "/fixtures/application/../app/helper.php\";
app_main();
echo json_encode(array_map(\"basename\", array_keys(\\opcov\\stop()))), \"\\n\";
report();";
echo opcov_child($script, ["opcov.collect_paths" => "$dir/linked"]);
unlink("$dir/linked");
rmdir($dir);
--EXPECT--
["helper.php","main.php"]
off: null
selected: app/main.php
selected: app/helper.php
skipped: report.inc: not in opcov.collect_paths
