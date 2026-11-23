--TEST--
Reports written by two processes with one test each parse with opcov-core and merge to the bytes one process with both tests writes
--SKIPIF--
<?php
if (!is_executable((string) getenv('OPCOV_CLI'))) {
    die('skip OPCOV_CLI does not name the opcov binary');
}
?>
--INI--
opcov.enabled=1
opcov.collect_paths={PWD}/fixtures/app
--FILE--
<?php
require __DIR__ . '/../support/coverage.inc';
require __DIR__ . '/fixtures/report.inc';
$app = __DIR__ . '/fixtures/app';
$dir = opcov_report_dir();
$run = static function (array $tests, string $out) use ($app): string {
    $script = '<?php require ' . var_export("$app/calc.php", true) . ";\n";
    foreach ($tests as $name => $call) {
        $script .= "\\opcov\\start(); $call; \\opcov\\stop(" . var_export($name, true) . "); \\opcov\\reset();\n";
    }
    $script .= '\opcov\write_report(' . var_export($out, true) . ', ["root" => ' . var_export($app, true) . "]);\n";
    return opcov_child($script, ['opcov.enabled' => '1', 'opcov.collect_paths' => $app]);
};
$add = ['Tests\CalcTest::test_add' => 'add(1, 2); sign(1)'];
$sign = ['Tests\CalcTest::test_sign' => 'sign(-1)'];
echo $run($add, "$dir/add.json"), $run($sign, "$dir/sign.json"), $run($sign + $add, "$dir/both.json");
$cli = escapeshellarg(getenv('OPCOV_CLI'));
passthru("$cli merge " . escapeshellarg("$dir/sign.json") . ' ' . escapeshellarg("$dir/add.json") . ' -o ' . escapeshellarg("$dir/merged.json"), $status);
var_dump($status);
var_dump(file_get_contents("$dir/merged.json") === file_get_contents("$dir/both.json"));
$merged = json_decode(file_get_contents("$dir/merged.json"), true);
echo json_encode([$merged['tests'], $merged['files']['calc.php']['lines']]), "\n";
opcov_remove_dir($dir);
--EXPECT--
int(0)
bool(true)
[["Tests\\CalcTest::test_add","Tests\\CalcTest::test_sign"],{"4":[0],"9":[0,1],"10":[1],"12":[0],"15":[],"16":[]}]
