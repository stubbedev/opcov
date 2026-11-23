--TEST--
The request may end while collecting; pending hits are discarded at request shutdown
--INI--
opcov.enabled=1
opcov.collect_paths={PWD}/fixtures
--FILE--
<?php
require __DIR__ . "/fixtures/work.php";
\opcov\start();
work(3);
register_shutdown_function(static function (): void {
    echo "shutdown: ", count(\opcov\stop()), " file\n";
    \opcov\start();
    work(2);
});
echo "end of script\n";
--EXPECT--
end of script
shutdown: 1 file
