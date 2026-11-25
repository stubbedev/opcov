<?php

namespace App\Models;

use Illuminate\Database\Eloquent\Builder;
use Illuminate\Database\Eloquent\Model;

class Comment extends Model
{
    /** @param Builder<Comment> $query */
    public function scopeVisible(Builder $query): void
    {
    }

    public function approve(): void
    {
    }
}
