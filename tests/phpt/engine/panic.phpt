--TEST--
A panic at an engine entry point keeps PHP running and disables collection with the reason
--FILE--
<?php
\opcov\selftest_panic();
echo "still running\n";
(new ReflectionExtension('opcov'))->info();
--EXPECTF--
%Adeliberate panic injected by opcov\selftest_panic()
%Astill running

opcov

Version => %d.%d.%d
Built for => PHP 8.%d.%s %r(ZTS|NTS)%r, API %d
Running on => PHP 8.%d.%s, SAPI cli
VM kind => %r(CALL|SWITCH|GOTO|HYBRID|TAILCALL)%r
OPcache => %r(not loaded|off for the cli SAPI)%r
Conflicts => none
Collection => disabled: panic in opcov\selftest_panic(): deliberate panic injected by opcov\selftest_panic() (see https://github.com/stubbedev/opcov/blob/main/docs/troubleshooting.md#panic)

Directive => Local Value => Master Value
opcov.enabled => 0 => 0
opcov.collect_paths => no value => no value
opcov.exclude_paths => no value => no value
opcov.exclude_pattern => no value => no value
