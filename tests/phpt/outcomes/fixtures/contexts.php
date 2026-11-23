<?php
$closure = static function ($a) {
    return $a ?? 'fallback';
};

function generate($a)
{
    yield $a ?? 'fallback';
    if ($a) {
        yield 'then';
    }
}

function in_fiber($a)
{
    $fiber = new Fiber(static function ($a) {
        Fiber::suspend($a ?? 'fallback');
        return $a ? 'then' : 'else';
    });
    $fiber->start($a);
    $fiber->resume();
    return $fiber->getReturn();
}

function in_finally($a)
{
    try {
        return 'body';
    } finally {
        if ($a) {
            echo "finally then\n";
        }
    }
}

function guarded($a, $class)
{
    try {
        if ($a instanceof $class) {
            return 'yes';
        }
        return 'no';
    } catch (Error $e) {
        return 'error';
    }
}

function undefined_condition()
{
    try {
        if ($missing) {
            return 'then';
        }
        return 'else';
    } catch (ErrorException $e) {
        return 'caught';
    }
}

function iterate(iterable $items)
{
    foreach ($items as $item) {
        if ($item) {
            echo "item\n";
        }
    }
}

function throwing(): Generator
{
    yield 1;
    throw new RuntimeException('stop');
}

return $closure;
