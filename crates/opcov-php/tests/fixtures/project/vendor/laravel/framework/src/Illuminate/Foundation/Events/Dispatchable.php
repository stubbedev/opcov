<?php

namespace Illuminate\Foundation\Events;

trait Dispatchable
{
    public static function dispatch(mixed ...$arguments): void
    {
    }
}
