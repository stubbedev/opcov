<?php

namespace App\Models;

use Illuminate\Database\Eloquent\Builder;

/** @extends Builder<Post> */
class PostBuilder extends Builder
{
    public function published(): static
    {
        return $this;
    }
}
