<?php
function abort(string $how): void
{
    if ($how === 'exit') {
        echo "exiting\n";
        exit(3);
    }
    if ($how === 'fatal') {
        $how .= '!';
        function abort(): void {}
    }
    echo "not reached\n";
}
