--TEST--
Measured files compile without switch jump tables so every case is a comparison; unselected files keep them
--INI--
opcov.enabled=1
opcov.collect_paths={PWD}/fixtures/app
--FILE--
<?php
require __DIR__ . "/fixtures/app/switch.php";
require __DIR__ . "/fixtures/application/switch.php";
$info = \opcov\get_debug_info(__DIR__ . "/fixtures/app/switch.php");
$opcodes = [];
foreach ($info["functions"]["sw"]["function"]["blocks"] as $block) {
    foreach ($block["ops"] as $op) {
        $opcodes[$op["opcode"]] = true;
    }
}
var_dump(isset($opcodes["SWITCH_LONG"]), isset($opcodes["IS_EQUAL"]) || isset($opcodes["CASE"]));
echo \opcov\get_status()["counters"]["jump_table_files"], " files kept their jump tables: the test script and fixtures/application/switch.php\n";
\opcov\start();
sw(2);
echo implode(",", array_keys(\opcov\stop()[__DIR__ . "/fixtures/app/switch.php"]["lines"])), "\n";
--EXPECT--
bool(false)
bool(true)
2 files kept their jump tables: the test script and fixtures/application/switch.php
5,7,8
