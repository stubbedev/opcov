--TEST--
Names and file names that are not UTF-8 keep their bytes: the live API returns the raw file name and canonically encoded keys, reports carry the encoding, merge back to the bytes one process writes and convert (lcov with the raw path, Clover with the encoding)
--SKIPIF--
<?php
require __DIR__ . '/../support/coverage.inc';
if (!opcov_filesystem_takes_non_utf8_names()) {
    die('skip the file system refuses file names that are not UTF-8');
}
if (!is_executable((string) getenv('OPCOV_CLI'))) {
    die('skip OPCOV_CLI does not name the opcov binary');
}
?>
--FILE--
<?php
require __DIR__ . '/../support/coverage.inc';
require __DIR__ . '/fixtures/report.inc';
$dir = opcov_report_dir();
$file = "$dir/caf\xE9 100%.php";
file_put_contents($file, <<<PHP
<?php
namespace Caf\xE9 {
    function \xE9t\xE9(int \$x): int
    {
        return \$x > 0 ? 1 : 2;
    }
    class Ma\xEFs
    {
        public function r\xE9sum\xE9(bool \$b): string
        {
            if (\$b) {
                return 'y';
            }
            return 'n';
        }
    }
}
namespace Runner {
    function a(): void
    {
        \Caf\xE9\\\xE9t\xE9(1);
        (new \Caf\xE9\Ma\xEFs())->r\xE9sum\xE9(true);
    }
    function b(): void
    {
        \Caf\xE9\\\xE9t\xE9(-1);
        (new \Caf\xE9\Ma\xEFs())->r\xE9sum\xE9(false);
    }
}

PHP);
$show = static fn (string $text): string => addcslashes(str_replace($dir, 'DIR', $text), "\x80..\xFF");
$run = static function (array $tests, string $out, bool $describe) use ($dir, $file): string {
    $script = '<?php require ' . var_export($file, true) . ";\n";
    $script .= '$file = ' . var_export($file, true) . ";\n";
    $script .= '$show = static fn (string $text): string => addcslashes(str_replace(' . var_export($dir, true) . ", 'DIR', \$text), \"\\x80..\\xFF\");\n";
    foreach ($tests as $test) {
        $script .= "\\opcov\\start(); \\Runner\\$test(); \$hits = \\opcov\\stop('T::$test'); \\opcov\\reset();\n";
    }
    if ($describe) {
        $script .= <<<'PHP'
echo "stop() files: ", implode(', ', array_map($show, array_keys($hits))), "\n";
foreach (array_keys(\opcov\executable([$file])[$file]['functions']) as $key) {
    echo "  ", $show($key), "\n";
}
$status = \opcov\get_status();
echo "selected: ", $show(implode(', ', $status['selected_files'])), "\n";
echo "failed: ", json_encode($status['failed_files']), "\n";
var_dump($status['off_reason']);

PHP;
    }
    $script .= '\opcov\write_report(' . var_export($out, true) . ', ["root" => ' . var_export($dir, true) . "]);\n";
    return opcov_child($script, ['opcov.enabled' => '1', 'opcov.collect_paths' => $dir]);
};
echo $run(['a'], "$dir/a.json", true);
echo $run(['b'], "$dir/b.json", false), $run(['a', 'b'], "$dir/both.json", false);
$report = json_decode(file_get_contents("$dir/both.json"), true, flags: JSON_THROW_ON_ERROR);
echo "report files: ", implode(', ', array_keys($report['files'])), "\n";
foreach ($report['files']['caf%E9 100%25.php']['functions'] as $key => $function) {
    echo "  $key: ", count($function['outcomes']), " outcomes\n";
}
$merge = opcov_cli(['merge', "$dir/b.json", "$dir/a.json", '-o', "$dir/merged.json"]);
echo $merge->output;
var_dump($merge->status, file_get_contents("$dir/merged.json") === file_get_contents("$dir/both.json"));
foreach (['lcov', 'clover', 'html'] as $format) {
    $convert = opcov_cli(['convert', "$dir/both.json", '--format', $format, '-o', "$dir/out.$format"]);
    echo $convert->output, "$format: {$convert->status}\n";
}
$lcov = file_get_contents("$dir/out.lcov");
preg_match_all('/^SF:.*$/m', $lcov, $lines);
foreach ($lines[0] as $line) {
    echo $show($line), "\n";
}
preg_match('/<file name="([^"]*)"/', file_get_contents("$dir/out.clover"), $clover);
echo "clover: ", $show($clover[1]), "\n";
echo "html names the file: ", json_encode(str_contains(file_get_contents("$dir/out.html"), 'caf%E9 100%25.php')), "\n";
opcov_remove_dir($dir);
--EXPECT--
stop() files: DIR/caf\351 100%.php
  {main}@DIR/caf%E9 100%25.php
  Caf%E9\%E9t%E9
  Runner\a
  Runner\b
  Caf%E9\Ma%EFs::r%E9sum%E9
selected: DIR/caf%E9 100%25.php
failed: []
NULL
report files: caf%E9 100%25.php
  {main}@caf%E9 100%25.php: 0 outcomes
  Caf%E9\%E9t%E9: 2 outcomes
  Runner\a: 0 outcomes
  Runner\b: 0 outcomes
  Caf%E9\Ma%EFs::r%E9sum%E9: 2 outcomes
int(0)
bool(true)
lcov: 0
clover: 0
html: 0
SF:DIR/caf\351 100%.php
clover: DIR/caf%E9 100%25.php
html names the file: true
