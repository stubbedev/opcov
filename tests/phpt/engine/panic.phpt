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
PHP API => %d
Thread safety => %r(ZTS|NTS)%r
VM kind => %r(CALL|SWITCH|GOTO|HYBRID|TAILCALL)%r
Collection => disabled: panic in opcov\selftest_panic(): deliberate panic injected by opcov\selftest_panic()
