--TEST--
@listfile reads one path per line; a missing or empty listfile is refused with its path
--INI--
opcov.enabled=1
opcov.collect_paths={PWD}/fixtures/app
--FILE--
<?php
require __DIR__ . "/../support/coverage.inc";
$dir = sys_get_temp_dir() . "/opcov-listfile-" . getmypid();
mkdir($dir);
$list = "$dir/paths.txt";
file_put_contents($list, __DIR__ . "/fixtures/app/main.php\n\n" . __DIR__ . "/fixtures/application\n");
$script = "<?php require \"" . __DIR__ . "/fixtures/report.inc\"; require \"" . __DIR__ . "/fixtures/app/main.php\"; require \"" . __DIR__ . "/fixtures/app/helper.php\"; require \"" . __DIR__ . "/fixtures/application/main.php\"; report();";
echo opcov_child($script, ["opcov.collect_paths" => "@$list"]);
file_put_contents($list, "\n \n");
echo str_replace($dir, "DIR", opcov_child($script, ["opcov.collect_paths" => "@$list"]));
unlink($list);
echo str_replace($dir, "DIR", opcov_child($script, ["opcov.collect_paths" => "@$list"]));
rmdir($dir);
--EXPECT--
off: null
selected: app/main.php
selected: application/main.php
skipped: app/helper.php: not in opcov.collect_paths
skipped: report.inc: not in opcov.collect_paths
off: opcov.collect_paths: listfile DIR/paths.txt contains no paths (see https://github.com/stubbedev/opcov/blob/main/docs/troubleshooting.md#invalid-selection)
off: opcov.collect_paths: cannot read listfile DIR/paths.txt: No such file or directory (os error 2) (see https://github.com/stubbedev/opcov/blob/main/docs/troubleshooting.md#invalid-selection)
