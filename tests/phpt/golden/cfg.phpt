--TEST--
The CFG snapshots the extension builds match the hand-made goldens of opcov-core
--SKIPIF--
<?php
if (!is_dir(__DIR__ . '/../../../crates/opcov-core/tests/golden/cfg')) {
    die('skip the opcov-core goldens are not part of this test tree');
}
?>
--INI--
opcov.enabled=1
opcov.collect_paths={PWD}
--FILE--
<?php
require __DIR__ . '/../support/coverage.inc';
$script = <<<'PHP'
<?php
$golden = json_decode(file_get_contents(getenv('OPCOV_GOLDEN')), true, flags: JSON_THROW_ON_ERROR);
$file = getenv('OPCOV_GOLDEN_DIR') . '/' . $golden['input']['file'];
file_put_contents($file, $golden['source']);
include $file;
$prefix = dirname($file) . '/';
$actual = \opcov\get_debug_info($file)['functions'][$golden['input']['key']] ?? null;
$actual = json_decode(str_replace(trim(json_encode($prefix), '"'), '', json_encode($actual)), true);
unlink($file);
if ($actual == $golden['expected']) {
    echo "match\n";
    exit;
}
echo "expected: ", json_encode($golden['expected']), "\nactual:   ", json_encode($actual), "\n";
PHP;
$dir = sys_get_temp_dir() . '/opcov-golden-' . getmypid();
mkdir($dir);
$dir = realpath($dir);
putenv("OPCOV_GOLDEN_DIR=$dir");
foreach (glob(__DIR__ . '/../../../crates/opcov-core/tests/golden/cfg/*.json') as $golden) {
    putenv("OPCOV_GOLDEN=$golden");
    echo basename($golden, '.json'), ': ', opcov_child($script, ['opcov.collect_paths' => $dir]);
}
rmdir($dir);
--EXPECT--
and_or: match
coalesce: match
do_while: match
for: match
foreach: match
foreach_by_ref: match
if_elseif_else: match
if_without_else: match
match_chain_default: match
match_chain_no_default: match
match_jumptable: match
nested_loops_break: match
nullsafe: match
short_ternary: match
switch_expr_no_default: match
switch_fallthrough: match
try_catch_finally: match
while: match
