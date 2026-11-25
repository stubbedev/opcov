<?php

namespace App\Models;

use App\Events\CommentCreated;
use Illuminate\Database\Eloquent\Builder;
use Illuminate\Database\Eloquent\Model;

class Comment extends Model
{
    use MakesSlug;

    /** @var array<string, class-string> */
    protected $dispatchesEvents = ['created' => CommentCreated::class];

    /** @param Builder<Comment> $query */
    public function scopeVisible(Builder $query): void
    {
    }

    public function approve(): void
    {
    }
}
