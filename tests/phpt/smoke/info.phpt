--TEST--
ReflectionExtension::info(), which php --ri prints, shows the version, the PHP it was built for and runs on, the VM kind, the OPcache state, conflicts, the collection state and the INI settings
--FILE--
<?php
(new ReflectionExtension('opcov'))->info();
--EXPECTF--
opcov

Version => %d.%d.%d
Built for => PHP 8.%d.%s %r(ZTS|NTS)%r, API %d
Running on => PHP 8.%d.%s, SAPI cli
VM kind => %r(CALL|SWITCH|GOTO|HYBRID|TAILCALL)%r
OPcache => %r(not loaded|off for the cli SAPI)%r
Conflicts => none
Collection => off: opcov.enabled was 0 when PHP started; set opcov.enabled=1 in php.ini or with -d (see https://github.com/stubbedev/opcov/blob/main/docs/troubleshooting.md#not-enabled-at-startup)

Directive => Local Value => Master Value
opcov.enabled => 0 => 0
opcov.collect_paths => no value => no value
opcov.exclude_paths => no value => no value
opcov.exclude_pattern => no value => no value
