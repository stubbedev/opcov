<?php

namespace App\Models;

trait MakesSlug
{
    public static function bootMakesSlug(): void
    {
        static::creating(function (): void {
        });
    }
}
