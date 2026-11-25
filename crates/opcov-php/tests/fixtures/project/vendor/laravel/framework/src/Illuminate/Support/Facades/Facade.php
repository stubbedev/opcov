<?php

namespace Illuminate\Support\Facades;

abstract class Facade
{
    public static function defaultAliases(): array
    {
        return [
            'Artisan' => Artisan::class,
        ];
    }

    public static function __callStatic(string $method, array $arguments): mixed
    {
        return null;
    }
}
