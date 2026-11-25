<?php

namespace App\Uses;

use App\Models\Post;

final class Forwarded
{
    public function run(): void
    {
        Post::published();
    }
}
