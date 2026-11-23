<?php
function silenced(bool $warn): string
{
    if ($warn) {
        @trigger_error('screamed through @', E_USER_WARNING);
    }
    return 'done';
}
