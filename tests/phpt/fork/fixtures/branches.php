<?php
function branch(int $n): string
{
    if ($n === 0) {
        return 'zero';
    }
    if ($n === 1) {
        return 'one';
    }
    return 'other';
}
