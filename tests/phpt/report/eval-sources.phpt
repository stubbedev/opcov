--TEST--
Each distinct source eval()'d at one site is its own file, named <file>(<line>) : eval()'d code #<first 16 hex digits of its sha256>, in stop(), executable(), get_status() and write_report(); the same source eval()'d again is the same file
--INI--
opcov.enabled=1
opcov.collect_paths={PWD}/fixtures/evals
--FILE--
<?php
require __DIR__ . '/fixtures/report.inc';
$evals = __DIR__ . '/fixtures/evals';
require "$evals/mixin.php";
$one = 'return 1;';
$two = "\$x = 2;\nif (\$x > 1) {\n    return \$x;\n}\nreturn 0;";
$names = [
    "$evals/mixin.php(4) : eval()'d code #" . substr(hash('sha256', $one), 0, 16) => '{ONE}',
    "$evals/mixin.php(4) : eval()'d code #" . substr(hash('sha256', $two), 0, 16) => '{TWO}',
    "mixin.php(4) : eval()'d code #" . substr(hash('sha256', $one), 0, 16) => '{ONE}',
    "mixin.php(4) : eval()'d code #" . substr(hash('sha256', $two), 0, 16) => '{TWO}',
    "$evals/" => '',
];
$show = static function (array $hits) use ($names): void {
    $shown = [];
    foreach ($hits as $file => $hit) {
        $shown[] = strtr($file, $names) . ': lines ' . implode(',', array_keys($hit['lines'])) . '; functions [' . strtr(implode(',', array_keys($hit['functions'])), $names) . ']';
    }
    sort($shown);
    echo implode("\n", $shown), "\n";
};
\opcov\start();
var_dump(mixin($one));
$show(\opcov\stop('T::one'));
\opcov\reset();
\opcov\start();
var_dump(mixin($two), mixin($one));
$stopped = \opcov\stop('T::two');
$show($stopped);
\opcov\reset();
echo "executable:\n";
$show(\opcov\executable(array_keys($stopped)));
echo "selected:\n";
foreach (\opcov\get_status()['selected_files'] as $file) {
    echo strtr($file, $names), "\n";
}
$dir = opcov_report_dir();
\opcov\write_report("$dir/report.json", ['root' => $evals]);
$report = json_decode(file_get_contents("$dir/report.json"), true);
echo "report:\n";
$shown = [];
foreach ($report['files'] as $file => $entry) {
    $source = match (strtr($file, $names)) {
        '{ONE}' => $one,
        '{TWO}' => $two,
        default => file_get_contents("$evals/$file"),
    };
    $shown[] = strtr($file, $names) . ': sha256 ' . ($entry['sha256'] === hash('sha256', $source) ? 'of its source' : 'WRONG') . '; lines ' . json_encode($entry['lines']) . '; functions ' . strtr(implode(',', array_keys($entry['functions'])), $names);
}
sort($shown);
echo implode("\n", $shown), "\n";
opcov_remove_dir($dir);
--EXPECT--
int(1)
mixin.php: lines 4; functions []
{ONE}: lines 1; functions []
int(2)
int(1)
mixin.php: lines 4; functions []
{ONE}: lines 1; functions []
{TWO}: lines 1,2,3; functions [{main}@{TWO}]
executable:
mixin.php: lines 4; functions [{main}@mixin.php,mixin]
{ONE}: lines 1; functions [{main}@{ONE}]
{TWO}: lines 1,2,3,5; functions [{main}@{TWO}]
selected:
mixin.php
{ONE}
{TWO}
report:
mixin.php: sha256 of its source; lines {"4":[0,1]}; functions {main}@mixin.php,mixin
{ONE}: sha256 of its source; lines {"1":[0,1]}; functions {main}@{ONE}
{TWO}: sha256 of its source; lines {"1":[1],"2":[1],"3":[1],"5":[]}; functions {main}@{TWO}
