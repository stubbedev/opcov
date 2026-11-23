--TEST--
ReflectionExtension::info(), which php --ri prints, shows version, PHP API, thread safety, VM kind, collection state and the INI settings
--FILE--
<?php
(new ReflectionExtension('opcov'))->info();
--EXPECTF--
opcov

Version => %d.%d.%d
PHP API => %d
Thread safety => %r(ZTS|NTS)%r
VM kind => %r(CALL|SWITCH|GOTO|HYBRID|TAILCALL)%r
Collection => off: opcov.enabled was 0 when PHP started; set opcov.enabled=1 in php.ini or with -d

Directive => Local Value => Master Value
opcov.enabled => 0 => 0
opcov.collect_paths => no value => no value
opcov.exclude_paths => no value => no value
opcov.exclude_pattern => no value => no value
