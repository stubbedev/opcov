--TEST--
ReflectionExtension::info(), which php --ri prints, shows version, PHP API, thread safety, VM kind and collection state
--FILE--
<?php
(new ReflectionExtension('opcov'))->info();
--EXPECTF--
opcov

Version => %d.%d.%d
PHP API => %d
Thread safety => %r(ZTS|NTS)%r
VM kind => %r(CALL|SWITCH|GOTO|HYBRID|TAILCALL)%r
Collection => available
