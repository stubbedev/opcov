--TEST--
Xdebug and PCOV are refused only in their coverage modes: Xdebug with coverage in xdebug.mode or in XDEBUG_MODE (which wins over the INI setting), PCOV with pcov.enabled=1; loaded in other modes they coexist with opcov
--SKIPIF--
<?php
if ((string) getenv('OPCOV_XDEBUG_EXTENSION') === '' || (string) getenv('OPCOV_PCOV_EXTENSION') === '') {
    die('skip OPCOV_XDEBUG_EXTENSION and OPCOV_PCOV_EXTENSION must name the Xdebug and PCOV builds for this PHP');
}
?>
--INI--
opcov.enabled=1
opcov.collect_paths={PWD}/fixtures
--FILE--
<?php
require __DIR__ . '/../support/coverage.inc';
$support = __DIR__ . '/../support/coverage.inc';
$silenced = __DIR__ . '/fixtures/silenced.php';
$script = <<<PHP
<?php
require '$support';
echo opcov_refusal();
echo json_encode(array_column(\opcov\get_status()['conflicts'], 'code')), "\n";
if (\opcov\get_status()['off_code'] === null) {
    require '$silenced';
    echo opcov_lines('$silenced', static fn () => silenced(false));
}
PHP;
$xdebug = ['zend_extension' => getenv('OPCOV_XDEBUG_EXTENSION')];
$pcov = ['extension' => getenv('OPCOV_PCOV_EXTENSION')];
echo "== xdebug.mode=coverage\n", opcov_child($script, $xdebug + ['xdebug.mode' => 'coverage']);
echo "== xdebug.mode=develop,coverage\n", opcov_child($script, $xdebug + ['xdebug.mode' => 'develop,coverage']);
echo "== XDEBUG_MODE=coverage over xdebug.mode=off\n", opcov_child($script, $xdebug + ['xdebug.mode' => 'off'], null, ['XDEBUG_MODE' => 'coverage']);
echo "== xdebug.mode=develop,debug\n", opcov_child($script, $xdebug + ['xdebug.mode' => 'develop,debug', 'xdebug.start_with_request' => 'no']);
echo "== pcov.enabled=1\n", opcov_child($script, $pcov + ['pcov.enabled' => '1']);
echo "== pcov.enabled=0\n", opcov_child($script, $pcov + ['pcov.enabled' => '0']);
?>
--EXPECT--
== xdebug.mode=coverage
opcov cannot collect coverage: Xdebug is loaded with mode "coverage" (from xdebug.mode), which includes coverage: two coverage tools in one process measure each other; set xdebug.mode to modes without coverage (for example xdebug.mode=off), or do not load Xdebug [xdebug-coverage]
["xdebug-coverage"]
== xdebug.mode=develop,coverage
opcov cannot collect coverage: Xdebug is loaded with mode "develop,coverage" (from xdebug.mode), which includes coverage: two coverage tools in one process measure each other; set xdebug.mode to modes without coverage (for example xdebug.mode=off), or do not load Xdebug [xdebug-coverage]
["xdebug-coverage"]
== XDEBUG_MODE=coverage over xdebug.mode=off
opcov cannot collect coverage: Xdebug is loaded with mode "coverage" (from the XDEBUG_MODE environment variable), which includes coverage: two coverage tools in one process measure each other; set XDEBUG_MODE to modes without coverage (for example XDEBUG_MODE=off), or do not load Xdebug [xdebug-coverage]
["xdebug-coverage"]
== xdebug.mode=develop,debug
can collect
[]
silenced.php
  covered: 4,7
  uncovered: 5
== pcov.enabled=1
opcov cannot collect coverage: PCOV is loaded with pcov.enabled=1: two coverage tools in one process measure each other, and PCOV's instruction loop calls every probe again; set pcov.enabled=0, or do not load PCOV [pcov-enabled]
["pcov-enabled"]
== pcov.enabled=0
can collect
[]
silenced.php
  covered: 4,7
  uncovered: 5
