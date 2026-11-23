<?php
namespace N;
function f(string $s): string {
    return str_replace(' ', '', preg_replace_callback('/./', static function ($m) {
        if ($m[0] === 'a') {
            return 'x';
        }
        return 'y';
    }, $s));
}
