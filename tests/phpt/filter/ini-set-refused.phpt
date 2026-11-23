--TEST--
The INI settings are PHP_INI_SYSTEM|PHP_INI_PERDIR: ini_set() at runtime is refused
--INI--
opcov.enabled=1
opcov.collect_paths={PWD}/fixtures/app
--FILE--
<?php
foreach (["opcov.enabled" => "0", "opcov.collect_paths" => "/", "opcov.exclude_paths" => "/", "opcov.exclude_pattern" => "~x~"] as $name => $value) {
    var_dump(ini_set($name, $value));
}
echo ini_get("opcov.collect_paths") === __DIR__ . "/fixtures/app" ? "unchanged\n" : "changed\n";
--EXPECT--
bool(false)
bool(false)
bool(false)
bool(false)
unchanged
