--TEST--
The CFG snapshots the extension builds match the goldens of opcov-core recorded for this PHP version
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
$prefix = trim(json_encode(dirname($file) . '/'), '"');
$functions = json_decode(str_replace($prefix, '', json_encode(\opcov\get_debug_info($file)['functions'])), true);
$actual = $functions[$golden['input']['key']] ?? null;
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
$running = PHP_MAJOR_VERSION . '.' . PHP_MINOR_VERSION;
foreach (glob(__DIR__ . '/../../../crates/opcov-core/tests/golden/cfg/*.json') as $golden) {
    $versions = json_decode(file_get_contents($golden), true, flags: JSON_THROW_ON_ERROR)['php'];
    $minors = array_map(static fn (string $version): string => implode('.', array_slice(explode('.', $version), 0, 2)), $versions);
    if (!in_array($running, $minors, true)) {
        continue;
    }
    putenv("OPCOV_GOLDEN=$golden");
    $settings = ['opcov.collect_paths' => $dir, 'error_reporting' => (string) (E_ALL & ~E_DEPRECATED)];
    echo explode('.', basename($golden), 2)[0], ': ', opcov_child($script, $settings);
}
rmdir($dir);
--EXPECT--
and_or: match
arrow_function: match
break_continue_n: match
coalesce: match
do_while: match
enum_method: match
first_class_callable: match
for: match
foreach: match
foreach_by_ref: match
foreach_generator: match
foreach_iterator: match
generator: match
if_elseif_else: match
if_without_else: match
match_chain_default: match
match_chain_no_default: match
match_jumptable: match
nested_loops_break: match
nullsafe: match
property_hook: match
readonly_class: match
return_in_finally: match
short_ternary: match
static_closure: match
switch_expr_no_default: match
switch_fallthrough: match
try_catch_finally: match
while: match
yield_from: match
