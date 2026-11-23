--TEST--
User opcode handler chaining with a real extension: Xdebug's develop mode registers handlers for BEGIN_SILENCE and END_SILENCE before opcov; opcov forwards to them, so xdebug.scream still defeats @, and the lines are measured
--SKIPIF--
<?php
if ((string) getenv('OPCOV_XDEBUG_EXTENSION') === '') {
    die('skip OPCOV_XDEBUG_EXTENSION must name the Xdebug build for this PHP');
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
require '$silenced';
echo opcov_lines('$silenced', static fn () => silenced(true));
echo opcov_lines('$silenced', static fn () => silenced(true));
PHP;
echo opcov_child($script, ['zend_extension' => getenv('OPCOV_XDEBUG_EXTENSION'), 'xdebug.mode' => 'develop', 'xdebug.scream' => '1', 'html_errors' => '0', 'xdebug.cli_color' => '0']);
?>
--EXPECTF--
SCREAM:  Error suppression ignored for
Warning: screamed through @ in %ssilenced.php on line 5
%A
silenced.php
  covered: 4,5,7
  uncovered: 

SCREAM:  Error suppression ignored for
Warning: screamed through @ in %ssilenced.php on line 5
%A
silenced.php
  covered: 4,5,7
  uncovered: 
