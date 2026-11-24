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
trait Wraps {
    private function wrap(string $item): string {
        return "[$item]";
    }
}
final class Joiner {
    use Wraps;

    public function join(array $items): string {
        return implode(',', array_map(function ($item) { return $this->wrap($item); }, $items));
    }
}
function g(array $items): string {
    return implode(',', array_map(static function ($item) {
        return preg_replace_callback('/./', static fn ($m) => "<$m[0]>", $item);
    }, $items));
}
