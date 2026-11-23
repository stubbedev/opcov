<?php
function abort(string $how): void
{
    if ($how === 'exit') {
        echo "exiting\n";
        exit(3);
    }
    if ($how === 'fatal') {
        $blob = str_repeat('x', 256 * 1024 * 1024);
    }
    echo "not reached\n";
}
